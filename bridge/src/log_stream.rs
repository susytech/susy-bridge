// Copyleft 2017 Superstring.Community
// This file is part of Susy-Bridge.

// Susy-Bridge is free software: you can redistribute it and/or modify
// it under the terms of the GNU General Public License as published by
// the Free Software Foundation, either version 3 of the License, or
// (at your option) any later version.

// Susy-Bridge is distributed in the hope that it will be useful,
// but WITHOUT ANY WARRANTY; without even the implied warranty of
// MSRCHANTABILITY or FITNESS FOR A PARTICULAR PURPOSE.  See the
// GNU General Public License for more details.

// You should have received a copy of the GNU General Public License
// along with Susy-Bridge.  If not, see <http://www.gnu.org/licenses/>.

use error::{self, ResultExt};
use sofabi;
use futures::future::FromErr;
use futures::{Async, Future, Poll, Stream};
use std::time::Duration;
use tokio_timer::{Timeout, Timer};
use susyweb;
use susyweb::api::Namespace;
use susyweb::helpers::CallFuture;
use susyweb::types::{Address, FilterBuilder, H256, Log};
use susyweb::Transport;
use block_number_stream::{BlockNumberStream, BlockNumberStreamOptions};

fn sofabi_topic_to_susyweb(topic: &sofabi::Topic<sofabi::Hash>) -> Option<Vec<H256>> {
    match topic {
        sofabi::Topic::Any => None,
        sofabi::Topic::OneOf(options) => Some(options.clone()),
        sofabi::Topic::This(hash) => Some(vec![hash.clone()]),
    }
}

fn filter_to_builder(filter: &sofabi::TopicFilter, address: Address) -> FilterBuilder {
    let t0 = sofabi_topic_to_susyweb(&filter.topic0);
    let t1 = sofabi_topic_to_susyweb(&filter.topic1);
    let t2 = sofabi_topic_to_susyweb(&filter.topic2);
    let t3 = sofabi_topic_to_susyweb(&filter.topic3);
    FilterBuilder::default()
        .address(vec![address])
        .topics(t0, t1, t2, t3)
}

/// options for creating a `LogStream`. passed to `LogStream::new`
pub struct LogStreamOptions<T> {
    pub filter: sofabi::TopicFilter,
    pub request_timeout: Duration,
    pub poll_interval: Duration,
    pub confirmations: u32,
    pub transport: T,
    pub contract_address: Address,
    pub after: u64,
}

/// Contains all logs matching `LogStream` filter in inclusive block range `[from, to]`.
#[derive(Debug, PartialEq)]
pub struct LogsInBlockRange {
    pub from: u64,
    pub to: u64,
    pub logs: Vec<Log>,
}

/// Log Stream state.
enum State<T: Transport> {
    /// Fetching best block number.
    AwaitBlockNumber,
    /// Fetching logs for new best block.
    AwaitLogs {
        from: u64,
        to: u64,
        future: Timeout<FromErr<CallFuture<Vec<Log>, T::Out>, error::Error>>,
    },
}

/// `Stream` that repeatedly polls logs matching `filter_builder` from `contract_address`
/// with adjustable `poll_interval` and `request_timeout`.
/// yields new logs that are `confirmations` blocks deep.
pub struct LogStream<T: Transport> {
    block_number_stream: BlockNumberStream<T>,
    request_timeout: Duration,
    transport: T,
    last_checked_block: u64,
    timer: Timer,
    state: State<T>,
    filter_builder: FilterBuilder,
    topic: Vec<H256>,
}

impl<T: Transport> LogStream<T> {
    pub fn new(options: LogStreamOptions<T>) -> Self {
        let timer = Timer::default();

        let topic = sofabi_topic_to_susyweb(&options.filter.topic0)
            .expect("filter must have at least 1 topic. q.e.d.");
        let filter_builder = filter_to_builder(&options.filter, options.contract_address);

        let block_number_stream_options = BlockNumberStreamOptions {
            request_timeout: options.request_timeout,
            poll_interval: options.poll_interval,
            confirmations: options.confirmations,
            transport: options.transport.clone(),
            after: options.after,
        };

        LogStream {
            block_number_stream: BlockNumberStream::new(block_number_stream_options),
            request_timeout: options.request_timeout,
            transport: options.transport,
            last_checked_block: options.after,
            timer,
            state: State::AwaitBlockNumber,
            filter_builder,
            topic,
        }
    }
}

impl<T: Transport> Stream for LogStream<T> {
    type Item = LogsInBlockRange;
    type Error = error::Error;

    fn poll(&mut self) -> Poll<Option<Self::Item>, Self::Error> {
        loop {
            let (next_state, value_to_yield) = match self.state {
                State::AwaitBlockNumber => {
                    let last_block = try_stream!(
                        self.block_number_stream
                            .poll()
                            .chain_err(|| "LogStream: fetching of last confirmed block number failed")
                    );
                    info!("LogStream: fetched confirmed block number {}", last_block);

                    let from = self.last_checked_block + 1;
                    let filter = self.filter_builder
                        .clone()
                        .from_block(from.into())
                        .to_block(last_block.into())
                        .build();
                    let future = susyweb::api::Sof::new(&self.transport).logs(filter);

                    info!(
                        "LogStream: fetching logs in blocks {} to {}",
                        from, last_block
                    );

                    let next_state = State::AwaitLogs {
                        from: from,
                        to: last_block,
                        future: self.timer.timeout(future.from_err(), self.request_timeout),
                    };

                    (next_state, None)
                }
                State::AwaitLogs {
                    ref mut future,
                    from,
                    to,
                } => {
                    let logs = try_ready!(
                        future
                            .poll()
                            .chain_err(|| "LogStream: polling susyweb logs failed")
                    );
                    info!(
                        "LogStream (topic: {:?}): fetched {} logs from block {} to block {}",
                        self.topic,
                        logs.len(),
                        from,
                        to
                    );
                    let log_range_to_yield = LogsInBlockRange { from, to, logs };

                    self.last_checked_block = to;
                    (State::AwaitBlockNumber, Some(log_range_to_yield))
                }
            };

            self.state = next_state;

            if value_to_yield.is_some() {
                return Ok(Async::Ready(value_to_yield));
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use contracts;
    use rustc_hex::FromHex;
    use tokio_core::reactor::Core;
    use susyweb::types::{Bytes, Log};

    #[test]
    fn test_log_stream_twice_no_logs() {
        let deposit_topic = contracts::main::events::relay_message::filter().topic0;

        let transport = mock_transport!(
            "sof_blockNumber" =>
                req => json!([]),
                res => json!("0x1011");
            "sof_getLogs" =>
                req => json!([{
                    "address": "0x0000000000000000000000000000000000000001",
                    "fromBlock": "0x4",
                    "toBlock": "0x1005",
                    "topics": [deposit_topic]
                }]),
                res => json!([]);
            "sof_blockNumber" =>
                req => json!([]),
                res => json!("0x1012");
            "sof_getLogs" =>
                req => json!([{
                    "address": "0x0000000000000000000000000000000000000001",
                    "fromBlock": "0x1006",
                    "toBlock": "0x1006",
                    "topics": [deposit_topic]
                }]),
                res => json!([]);
        );

        let log_stream = LogStream::new(LogStreamOptions {
            request_timeout: Duration::from_secs(1),
            poll_interval: Duration::from_secs(1),
            confirmations: 12,
            transport: transport.clone(),
            contract_address: "0000000000000000000000000000000000000001".into(),
            after: 3,
            filter: contracts::main::events::relay_message::filter(),
        });

        let mut event_loop = Core::new().unwrap();
        let log_ranges = event_loop.run(log_stream.take(2).collect()).unwrap();

        assert_eq!(
            log_ranges,
            vec![
                LogsInBlockRange {
                    from: 4,
                    to: 4101,
                    logs: vec![],
                },
                LogsInBlockRange {
                    from: 4102,
                    to: 4102,
                    logs: vec![],
                },
            ]
        );
        assert_eq!(transport.actual_requests(), transport.expected_requests());
    }

    #[test]
    fn test_log_stream_once_one_log() {
        let deposit_topic = contracts::main::events::relay_message::filter().topic0;

        let transport = mock_transport!(
            "sof_blockNumber" =>
                req => json!([]),
                res => json!("0x1011");
            "sof_getLogs" =>
                req => json!([{
                    "address": "0x0000000000000000000000000000000000000001",
                    "fromBlock": "0x4",
                    "toBlock": "0x1005",
                    "topics": [deposit_topic],
                }]),
                res => json!([{
                    "address": "0x0000000000000000000000000000000000000cc1",
                    "topics": [deposit_topic],
                    "data": "0x000000000000000000000000aff3454fce5edbc8cca8697c15331677e6ebcccc00000000000000000000000000000000000000000000000000000000000000f0",
                    "type": "",
                    "transactionHash": "0x884edad9ce6fa2440d8a54cc123490eb96d2768479d49ff9c7366125a9424364"
                }]);
        );

        let log_stream = LogStream::new(LogStreamOptions {
            request_timeout: Duration::from_secs(1),
            poll_interval: Duration::from_secs(1),
            confirmations: 12,
            transport: transport.clone(),
            contract_address: "0000000000000000000000000000000000000001".into(),
            after: 3,
            filter: contracts::main::events::relay_message::filter(),
        });

        let mut event_loop = Core::new().unwrap();
        let log_ranges = event_loop.run(log_stream.take(1).collect()).unwrap();

        assert_eq!(
            log_ranges,
            vec![
                LogsInBlockRange { from: 4, to: 4101, logs: vec![
                    Log {
                        address: "0x0000000000000000000000000000000000000cc1".into(),
                        topics: deposit_topic.into(),
                        data: Bytes("000000000000000000000000aff3454fce5edbc8cca8697c15331677e6ebcccc00000000000000000000000000000000000000000000000000000000000000f0".from_hex().unwrap()),
                        transaction_hash: Some("0x884edad9ce6fa2440d8a54cc123490eb96d2768479d49ff9c7366125a9424364".into()),
                        block_hash: None,
                        block_number: None,
                        transaction_index: None,
                        log_index: None,
                        transaction_log_index: None,
                        log_type: None,
                        removed: None,
                    }
                ] },
            ]);
        assert_eq!(transport.actual_requests(), transport.expected_requests());
    }
}
