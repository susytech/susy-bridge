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

use contracts;
use error::{self, ResultExt};
use futures::future::JoinAll;
use futures::{Async, Future, Poll};
use helpers;
use helpers::{AsyncCall, AsyncTransaction};
use main_contract::MainContract;
use message_to_main::MessageToMain;
use relay_stream::LogToFuture;
use side_contract::SideContract;
use signature::Signature;
use susyweb::types::{H256, Log};
use susyweb::Transport;

enum State<T: Transport> {
    AwaitMessage(AsyncCall<T, contracts::side::functions::message::Decoder>),
    AwaitIsRelayed {
        future: AsyncCall<T, contracts::main::functions::accepted_messages::Decoder>,
        message: MessageToMain,
    },
    AwaitSignatures {
        future: JoinAll<Vec<AsyncCall<T, contracts::side::functions::signature::Decoder>>>,
        message: MessageToMain,
    },
    AwaitMessageData {
        future: AsyncCall<T, contracts::side::functions::relayed_messages::Decoder>,
        message: MessageToMain,
        signatures: Vec<Signature>,
    },
    AwaitTxSent(AsyncTransaction<T>),
}

/// `Future` that completes a transfer from side to main by calling
/// `mainContract.withdraw` for a single `sideContract.CollectedSignatures`
/// these get created by the `side_to_main_signatures` `RelayStream` that's part
/// of the `Bridge`.
pub struct SideToMainSignatures<T: Transport> {
    side_tx_hash: H256,
    main: MainContract<T>,
    side: SideContract<T>,
    state: State<T>,
}

impl<T: Transport> SideToMainSignatures<T> {
    pub fn new(raw_log: &Log, main: MainContract<T>, side: SideContract<T>) -> Self {
        let side_tx_hash = raw_log
            .transaction_hash
            .expect("`log` must be mined and contain `transaction_hash`. q.e.d.");

        let log = helpers::parse_log(contracts::side::events::signed_message::parse_log, raw_log)
            .expect("`Log` must be a from a `CollectedSignatures` event. q.e.d.");

        // authority_responsible_for_relay is an indexed topic and it should be
        // always set up when creating the filter, so we receive only logs that
        // we should relay
        assert_eq!(
            log.authority_responsible_for_relay, main.authority_address,
            "incorrectly set up collected_signatures filter, we should only received logs where authority_responsible_for_relay == main.authority_address; qed"
        );

        info!("{:?} - step 1/3 - about to fetch message", side_tx_hash,);
        let (payload, decoder) = contracts::side::functions::message::call(log.message_hash);
        let state = State::AwaitMessage(side.call(payload, decoder));

        Self {
            side_tx_hash,
            main,
            side,
            state,
        }
    }
}

impl<T: Transport> Future for SideToMainSignatures<T> {
    type Item = Option<H256>;
    type Error = error::Error;

    fn poll(&mut self) -> Poll<Self::Item, Self::Error> {
        loop {
            let next_state = match self.state {
                State::AwaitMessage(ref mut future) => {
                    let message_bytes = try_ready!(
                        future
                            .poll()
                            .chain_err(|| "SubmitSignature: fetching message failed")
                    );
                    let message = MessageToMain::from_bytes(&message_bytes)?;

                    let (payload, decoder) = contracts::main::functions::accepted_messages::call(message.keccak256());
                    State::AwaitIsRelayed {
                        future: self.main.call(payload, decoder),
                        message,
                    }

                },
                State::AwaitIsRelayed { ref mut future, ref message } => {
                    let is_relayed = try_ready!(
                        future
                            .poll()
                            .chain_err(|| "SubmitSignature: fetching message failed")
                    );

                    if is_relayed {
                        return Ok(Async::Ready(None));
                    }

                    State::AwaitSignatures {
                        future: self.side.get_signatures(message.keccak256()),
                        message: message.clone(),
                    }
                },
                State::AwaitSignatures { ref mut future, ref message } => {
                    let raw_signatures = try_ready!(
                        future
                            .poll()
                            .chain_err(|| "WithdrawRelay: fetching message and signatures failed")
                    );
                    let signatures: Vec<Signature> = raw_signatures
                        .iter()
                        .map(|x| Signature::from_bytes(x))
                        .collect::<Result<_, _>>()?;
                    info!("{:?} - step 2/3 - message and {} signatures received. about to send transaction", self.side_tx_hash, signatures.len());

                    let (payload, decoder) = contracts::side::functions::relayed_messages::call(message.message_id);
                    State::AwaitMessageData {
                        future: self.side.call(payload, decoder),
                        message: message.clone(),
                        signatures,
                    }
                },
                State::AwaitMessageData { ref mut future, ref message, ref signatures } => {
                    let message_data = try_ready!(
                        future
                            .poll()
                            .chain_err(|| "SubmitSignature: fetching message failed")
                    );

                    State::AwaitTxSent(self.main.relay_side_to_main(&message, &signatures, message_data))
                },
                State::AwaitTxSent(ref mut future) => {
                    let main_tx_hash = try_ready!(
                        future
                            .poll()
                            .chain_err(|| "WithdrawRelay: sending transaction failed")
                    );
                    info!(
                        "{:?} - step 3/3 - DONE - transaction sent {:?}",
                        self.side_tx_hash, main_tx_hash
                    );
                    return Ok(Async::Ready(Some(main_tx_hash)));
                }
            };
            self.state = next_state;
        }
    }
}

/// options for relays from side to main
pub struct LogToSideToMainSignatures<T> {
    pub main: MainContract<T>,
    pub side: SideContract<T>,
}

/// from the options and a log a relay future can be made
impl<T: Transport> LogToFuture for LogToSideToMainSignatures<T> {
    type Future = SideToMainSignatures<T>;

    fn log_to_future(&self, log: &Log) -> Self::Future {
        SideToMainSignatures::new(log, self.main.clone(), self.side.clone())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use contracts;
    use sofabi;
    use rustc_hex::FromHex;
    use rustc_hex::ToHex;
    use tokio_core::reactor::Core;
    use susyweb::types::{Address, Bytes, Log};

    #[test]
    fn test_side_to_main_sign_relay_future_not_relayed_authority_responsible() {
        let authority_address: Address = "0000000000000000000000000000000000000001".into();
        let authority_responsible_for_relay = authority_address;
        let topic = contracts::side::events::signed_message::filter(authority_responsible_for_relay);

        let message = MessageToMain {
            side_tx_hash: "0x884edad9ce6fa2440d8a54cc123490eb96d2768479d49ff9c7366125a9424364".into(),
            message_id: "0x884edad9ce6fa2440d8a54cc123490eb96d2768479d49ff9c7366125a94243ff".into(),
            sender: "aff3454fce5edbc8cca8697c15331677e6ebccff".into(),
            recipient: "aff3454fce5edbc8cca8697c15331677e6ebcccc".into(),
        };

        let log = contracts::side::logs::SignedMessage {
            authority_responsible_for_relay,
            message_hash: message.keccak256(),
        };

        // TODO [snd] would be nice if sofabi derived log structs implemented `encode`
        let log_data = sofabi::encode(&[
            sofabi::Token::FixedBytes(log.message_hash.to_vec()),
        ]);

        let log_tx_hash: H256 =
            "0x884edad9ce6fa2440d8a54cc123490eb96d2768479d49ff9c7366125a9424364".into();

        let raw_log = Log {
            address: "0000000000000000000000000000000000000001".into(),
            topics: vec![topic.topic0[0], topic.topic1[0]],
            data: Bytes(log_data),
            transaction_hash: Some(log_tx_hash),
            block_hash: None,
            block_number: None,
            transaction_index: None,
            log_index: None,
            transaction_log_index: None,
            log_type: None,
            removed: None,
        };

        let side_contract_address: Address = "0000000000000000000000000000000000000dd1".into();
        let main_contract_address: Address = "0000000000000000000000000000000000000fff".into();

        let signature = Signature::from_bytes("8697c15331677e6ebccccaff3454fce5edbc8cca8697c15331677aff3454fce5edbc8cca8697c15331677e6ebccccaff3454fce5edbc8cca8697c15331677e6ebc".from_hex().unwrap().as_slice()).unwrap();

        let tx_hash = "0x1db8f385535c0d178b8f40016048f3a3cffee8f94e68978ea4b277f57b638f0b";
        let data: Vec<u8> = vec![10, 0];

        let main_transport = mock_transport!(
            "sof_call" =>
                req => json!([{
                    "data": format!("0x{}", contracts::main::functions::accepted_messages::encode_input(log.message_hash).to_hex()),
                    "to": main_contract_address,
                }, "latest"]),
                res => json!(format!("0x{}", sofabi::encode(&[sofabi::Token::Bool(false)]).to_hex()));
            "sof_sendTransaction" =>
                req => json!([{
                    "data": format!("0x{}",
                                    contracts::main::functions::accept_message::encode_input(
                                        vec![signature.v],
                                        vec![signature.r.clone()],
                                        vec![signature.s.clone()],
                                        message.side_tx_hash,
                                        data.clone(),
                                        message.sender,
                                        message.recipient,
                                    ).to_hex()),
                                    "from": format!("0x{}", authority_address.to_hex()),
                    "gas": "0xfd",
                    // TODO: fix gasPrice
                    "gasPrice": format!("0x{:x}", 1000),
                    "to": main_contract_address,
                }]),
                res => json!(tx_hash);
        );

        let side_transport = mock_transport!(
            "sof_call" =>
                req => json!([{
                    "data": format!("0x{}", contracts::side::functions::message::encode_input(log.message_hash).to_hex()),
                    "to": side_contract_address,
                }, "latest"]),
                res => json!(format!("0x{}", sofabi::encode(&[sofabi::Token::Bytes(message.to_bytes())]).to_hex()));
            "sof_call" =>
                req => json!([{
                    "data": format!("0x{}", contracts::side::functions::signature::encode_input(log.message_hash, 0).to_hex()),
                    "to": side_contract_address,
                }, "latest"]),
                res => json!(format!("0x{}", sofabi::encode(&[sofabi::Token::Bytes(signature.to_bytes())]).to_hex()));
            "sof_call" =>
                req => json!([{
                    "data": format!("0x{}", contracts::side::functions::relayed_messages::encode_input(message.message_id).to_hex()),
                    "to": side_contract_address,
                }, "latest"]),
                res => json!(format!("0x{}", sofabi::encode(&[sofabi::Token::Bytes(data)]).to_hex()));
        );

        let main_contract = MainContract {
            transport: main_transport.clone(),
            contract_address: main_contract_address,
            authority_address,
            request_timeout: ::std::time::Duration::from_millis(0),
            logs_poll_interval: ::std::time::Duration::from_millis(0),
            required_log_confirmations: 0,
            submit_collected_signatures_gas: 0xfd.into(),
        };

        let side_contract = SideContract {
            transport: side_transport.clone(),
            contract_address: side_contract_address,
            authority_address,
            required_signatures: 1,
            request_timeout: ::std::time::Duration::from_millis(0),
            logs_poll_interval: ::std::time::Duration::from_millis(0),
            required_log_confirmations: 0,
            sign_main_to_side_gas: 0.into(),
            sign_main_to_side_gas_price: 0.into(),
            sign_side_to_main_gas: 0xfd.into(),
            sign_side_to_main_gas_price: 0xa0.into(),
        };

        let future = SideToMainSignatures::new(&raw_log, main_contract, side_contract);

        let mut event_loop = Core::new().unwrap();
        let result = event_loop.run(future).unwrap();
        assert_eq!(result, Some(tx_hash.into()));

        assert_eq!(main_transport.actual_requests(), main_transport.expected_requests());
        assert_eq!(
            side_transport.actual_requests(),
            side_transport.expected_requests()
        );
    }

    #[test]
    fn test_side_to_main_sign_relay_future_already_relayed() {
        let authority_address: Address = "0000000000000000000000000000000000000001".into();
        let authority_responsible_for_relay = authority_address;
        let topic = contracts::side::events::signed_message::filter(authority_responsible_for_relay);

        let message = MessageToMain {
            side_tx_hash: "0x884edad9ce6fa2440d8a54cc123490eb96d2768479d49ff9c7366125a9424364".into(),
            message_id: "0x884edad9ce6fa2440d8a54cc123490eb96d2768479d49ff9c7366125a94243ff".into(),
            sender: "aff3454fce5edbc8cca8697c15331677e6ebccff".into(),
            recipient: "aff3454fce5edbc8cca8697c15331677e6ebcccc".into(),
        };

        let log = contracts::side::logs::SignedMessage {
            authority_responsible_for_relay,
            message_hash: message.keccak256(),
        };

        // TODO [snd] would be nice if sofabi derived log structs implemented `encode`
        let log_data = sofabi::encode(&[
            sofabi::Token::FixedBytes(log.message_hash.to_vec()),
        ]);

        let log_tx_hash: H256 =
            "0x884edad9ce6fa2440d8a54cc123490eb96d2768479d49ff9c7366125a9424364".into();

        let raw_log = Log {
            address: "0000000000000000000000000000000000000001".into(),
            topics: vec![topic.topic0[0], topic.topic1[0]],
            data: Bytes(log_data),
            transaction_hash: Some(log_tx_hash),
            block_hash: None,
            block_number: None,
            transaction_index: None,
            log_index: None,
            transaction_log_index: None,
            log_type: None,
            removed: None,
        };

        let side_contract_address: Address = "0000000000000000000000000000000000000dd1".into();
        let main_contract_address: Address = "0000000000000000000000000000000000000fff".into();

        let main_transport = mock_transport!(
            "sof_call" =>
                req => json!([{
                    "data": format!("0x{}", contracts::main::functions::accepted_messages::encode_input(log.message_hash).to_hex()),
                    "to": main_contract_address,
                }, "latest"]),
                res => json!(format!("0x{}", sofabi::encode(&[sofabi::Token::Bool(true)]).to_hex()));
        );

        let side_transport = mock_transport!(
            "sof_call" =>
                req => json!([{
                    "data": format!("0x{}", contracts::side::functions::message::encode_input(log.message_hash).to_hex()),
                    "to": side_contract_address,
                }, "latest"]),
                res => json!(format!("0x{}", sofabi::encode(&[sofabi::Token::Bytes(message.to_bytes())]).to_hex()));
        );

        let main_contract = MainContract {
            transport: main_transport.clone(),
            contract_address: main_contract_address,
            authority_address,
            request_timeout: ::std::time::Duration::from_millis(0),
            logs_poll_interval: ::std::time::Duration::from_millis(0),
            required_log_confirmations: 0,
            submit_collected_signatures_gas: 0xfd.into(),
        };

        let side_contract = SideContract {
            transport: side_transport.clone(),
            contract_address: side_contract_address,
            authority_address,
            required_signatures: 1,
            request_timeout: ::std::time::Duration::from_millis(0),
            logs_poll_interval: ::std::time::Duration::from_millis(0),
            required_log_confirmations: 0,
            sign_main_to_side_gas: 0.into(),
            sign_main_to_side_gas_price: 0.into(),
            sign_side_to_main_gas: 0xfd.into(),
            sign_side_to_main_gas_price: 0xa0.into(),
        };

        let future = SideToMainSignatures::new(&raw_log, main_contract, side_contract);

        let mut event_loop = Core::new().unwrap();
        let result = event_loop.run(future).unwrap();
        assert_eq!(result, None);

        assert_eq!(main_transport.actual_requests(), main_transport.expected_requests());
        assert_eq!(
            side_transport.actual_requests(),
            side_transport.expected_requests()
        );
    }
}
