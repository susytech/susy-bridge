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

//! spins up two susy nodes with the dev chain.
//! starts one bridge authority that connects the two.
//! does a deposit by sending sophy to the MainBridge.
//! asserts that the deposit got relayed to side chain.
//! does a withdraw by executing SideBridge.transferToMainViaRelay.
//! asserts that the withdraw got relayed to main chain.
extern crate bridge;
extern crate bridge_contracts;
extern crate sofabi;
extern crate sophon_types;
extern crate tempdir;
extern crate tokio_core;
extern crate susyweb;
extern crate rustc_hex;

use std::path::Path;
use std::process::Command;
use std::thread;
use std::time::Duration;

use tokio_core::reactor::Core;

use rustc_hex::FromHex;
use bridge::helpers::AsyncCall;
use susyweb::transports::http::Http;

const TMP_PATH: &str = "tmp";
const MAX_PARALLEL_REQUESTS: usize = 10;
const TIMEOUT: Duration = Duration::from_secs(1);

fn susy_main_command() -> Command {
    let mut command = Command::new("susy");
    command
        .arg("--base-path")
        .arg(format!("{}/main", TMP_PATH))
        .arg("--chain")
        .arg("dev")
        .arg("--no-ipc")
        .arg("--logging")
        .arg("rpc=trace")
        .arg("--susy-jsonrpc-port")
        .arg("8550")
        .arg("--susy-jsonrpc-apis")
        .arg("all")
        .arg("--port")
        .arg("30310")
        .arg("--gasprice")
        .arg("0")
        .arg("--reseal-min-period")
        .arg("0")
        .arg("--no-ws")
        .arg("--no-dapps")
        .arg("--no-warp")
        .arg("--no-ui");
    command
}

fn susy_side_command() -> Command {
    let mut command = Command::new("susy");
    command
        .arg("--base-path")
        .arg(format!("{}/side", TMP_PATH))
        .arg("--chain")
        .arg("dev")
        .arg("--no-ipc")
        .arg("--logging")
        .arg("rpc=trace")
        .arg("--susy-jsonrpc-port")
        .arg("8551")
        .arg("--susy-jsonrpc-apis")
        .arg("all")
        .arg("--port")
        .arg("30311")
        .arg("--gasprice")
        .arg("0")
        .arg("--reseal-min-period")
        .arg("0")
        .arg("--no-ws")
        .arg("--no-dapps")
        .arg("--no-warp")
        .arg("--no-ui");
    command
}

#[test]
fn test_basic_deposit_then_withdraw() {
    if Path::new(TMP_PATH).exists() {
        std::fs::remove_dir_all(TMP_PATH).expect("failed to remove tmp dir");
    }
    let _tmp_dir = tempdir::TempDir::new(TMP_PATH).expect("failed to create tmp dir");

    println!("\nbuild the deploy executable so we can run it later\n");
    assert!(
        Command::new("cargo")
            .env("RUST_BACKTRACE", "1")
            .current_dir("../deploy")
            .arg("build")
            .status()
            .expect("failed to build susy-bridge-deploy executable")
            .success()
    );

    println!("\nbuild the susy-bridge executable so we can run it later\n");
    assert!(
        Command::new("cargo")
            .env("RUST_BACKTRACE", "1")
            .current_dir("../cli")
            .arg("build")
            .status()
            .expect("failed to build susy-bridge executable")
            .success()
    );

    // start a susy node that represents the main chain
    let mut susy_main = susy_main_command()
        .spawn()
        .expect("failed to spawn susy main node");

    // start a susy node that represents the side chain
    let mut susy_side = susy_side_command()
        .spawn()
        .expect("failed to spawn susy side node");

    // give the clients time to start up
    thread::sleep(Duration::from_millis(3000));

    // A address containing a lot of tokens (0x00a329c0648769a73afac7f9381e08fb43dbea72) should be
    // automatically added with a password being an empty string.
    // source: https://susytech.github.io/wiki/Private-development-chain.html
    let user_address = "0x00a329c0648769a73afac7f9381e08fb43dbea72";
    let authority_address = "0x00bd138abd70e2f00903268f3db08f2d25677c9e";

    let main_contract_address = "0xebd3944af37ccc6b67ff61239ac4fef229c8f69f";
    let side_contract_address = "0xebd3944af37ccc6b67ff61239ac4fef229c8f69f";
    let main_recipient_address = "0xb4c79dab8f259c7aee6e5b2aa729821864227e84";
    let side_recipient_address = "0xb4c79dab8f259c7aee6e5b2aa729821864227e84";

    let data_to_relay_to_side = vec![0u8, 1, 5];
    let data_to_relay_to_main = vec![0u8, 1, 5, 7];

    // create authority account on main
    // this is currently not supported in susyweb crate so we have to use curl
    let exit_status = Command::new("curl")
		.arg("--data").arg(r#"{"jsonrpc":"2.0","method":"susy_newAccountFromPhrase","params":["node0", ""],"id":0}"#)
		.arg("-H").arg("Content-Type: application/json")
		.arg("-X").arg("POST")
		.arg("localhost:8550")
		.status()
		.expect("failed to create authority account on main");
    assert!(exit_status.success());
    // TODO [snd] assert that created address matches authority_address

    // TODO don't shell out to curl
    // create authority account on side
    // this is currently not supported in susyweb crate so we have to use curl
    let exit_status = Command::new("curl")
		.arg("--data").arg(r#"{"jsonrpc":"2.0","method":"susy_newAccountFromPhrase","params":["node0", ""],"id":0}"#)
		.arg("-H").arg("Content-Type: application/json")
		.arg("-X").arg("POST")
		.arg("localhost:8551")
		.status()
		.expect("failed to create/unlock authority account on side");
    assert!(exit_status.success());
    // TODO [snd] assert that created address matches authority_address

    // give the operations time to complete
    thread::sleep(Duration::from_millis(5000));

    // kill the clients so we can restart them with the accounts unlocked
    susy_main.kill().unwrap();
    susy_side.kill().unwrap();

    // wait for clients to shut down
    thread::sleep(Duration::from_millis(5000));

    // start a susy node that represents the main chain with accounts unlocked
    let mut susy_main = susy_main_command()
        .arg("--unlock")
        .arg(format!("{},{}", user_address, authority_address))
        .arg("--password")
        .arg("password.txt")
        .spawn()
        .expect("failed to spawn susy main node");

    // start a susy node that represents the side chain with accounts unlocked
    let mut susy_side = susy_side_command()
        .arg("--unlock")
        .arg(format!("{},{}", user_address, authority_address))
        .arg("--password")
        .arg("password.txt")
        .spawn()
        .expect("failed to spawn susy side node");

    // give nodes time to start up
    thread::sleep(Duration::from_millis(10000));

    // deploy bridge contracts

    println!("\ndeploying contracts\n");
    assert!(
        Command::new("env")
            .arg("RUST_BACKTRACE=1")
            .arg("../target/debug/susy-bridge-deploy")
            .env("RUST_LOG", "info")
            .arg("--config")
            .arg("bridge_config.toml")
            .arg("--database")
            .arg("tmp/bridge1_db.txt")
            .status()
            .expect("failed spawn susy-bridge-deploy")
            .success()
    );

    // start bridge authority 1
    let mut bridge1 = Command::new("env")
        .arg("RUST_BACKTRACE=1")
        .arg("../target/debug/susy-bridge")
        .env("RUST_LOG", "info")
        .arg("--config")
        .arg("bridge_config.toml")
        .arg("--database")
        .arg("tmp/bridge1_db.txt")
        .spawn()
        .expect("failed to spawn bridge process");

    let mut event_loop = Core::new().unwrap();

    // connect to main
    let main_transport = Http::with_event_loop(
        "http://localhost:8550",
        &event_loop.handle(),
        MAX_PARALLEL_REQUESTS,
    ).expect("failed to connect to main at http://localhost:8550");

    // connect to side
    let side_transport = Http::with_event_loop(
        "http://localhost:8551",
        &event_loop.handle(),
        MAX_PARALLEL_REQUESTS,
    ).expect("failed to connect to side at http://localhost:8551");

    println!("\ngive authority some funds to do relay later\n");

    event_loop
        .run(susyweb::confirm::send_transaction_with_confirmation(
            &main_transport,
            susyweb::types::TransactionRequest {
                from: user_address.into(),
                to: Some(authority_address.into()),
                gas: None,
                gas_price: None,
                value: Some(1000000000.into()),
                data: None,
                condition: None,
                nonce: None,
            },
            Duration::from_secs(1),
            0,
        ))
        .unwrap();

    event_loop
        .run(susyweb::confirm::send_transaction_with_confirmation(
            &side_transport,
            susyweb::types::TransactionRequest {
                from: user_address.into(),
                to: Some(authority_address.into()),
                gas: None,
                gas_price: None,
                value: Some(1000000000.into()),
                data: None,
                condition: None,
                nonce: None,
            },
            Duration::from_secs(1),
            0,
        ))
        .unwrap();

    println!("\ndeploy BridgeRecipient contracts\n");

    event_loop
        .run(susyweb::confirm::send_transaction_with_confirmation(
            &main_transport,
            susyweb::types::TransactionRequest {
                from: user_address.into(),
                to: None,
                gas: None,
                gas_price: None,
                value: None,
                data: Some(include_str!("../../compiled_contracts/RecipientTest.bin").from_hex().unwrap().into()),
                condition: None,
                nonce: None,
            },
            Duration::from_secs(1),
            0,
        ))
        .unwrap();

    event_loop
        .run(susyweb::confirm::send_transaction_with_confirmation(
            &side_transport,
            susyweb::types::TransactionRequest {
                from: user_address.into(),
                to: None,
                gas: None,
                gas_price: None,
                value: None,
                data: Some(include_str!("../../compiled_contracts/RecipientTest.bin").from_hex().unwrap().into()),
                condition: None,
                nonce: None,
            },
            Duration::from_secs(1),
            0,
        ))
        .unwrap();

    println!("\nSend the message to main chain and wait for the relay to side\n");

    let (payload, _) = bridge_contracts::main::functions::relay_message::call(data_to_relay_to_side.clone(), main_recipient_address);

    event_loop
        .run(susyweb::confirm::send_transaction_with_confirmation(
            &main_transport,
            susyweb::types::TransactionRequest {
                from: user_address.into(),
                to: Some(main_contract_address.into()),
                gas: None,
                gas_price: None,
                value: None,
                data: Some(payload.into()),
                condition: None,
                nonce: None,
            },
            Duration::from_secs(1),
            0,
        ))
        .unwrap();

    println!("\nSending message to main complete. Give it plenty of time to get mined and relayed\n");
    thread::sleep(Duration::from_millis(10000));

    let (payload, decoder) = bridge_contracts::test::functions::last_data::call();

    let response = event_loop
        .run(AsyncCall::new(
            &side_transport,
            side_recipient_address.into(),
            TIMEOUT,
            payload,
            decoder,
        ))
        .unwrap();

    assert_eq!(
        response,
        data_to_relay_to_side,
        "data was not relayed properly to the side chain"
    );

    println!("\nSend the message to side chain and wait for the relay to main\n");

    let (payload, _) = bridge_contracts::side::functions::relay_message::call(data_to_relay_to_main.clone(), main_recipient_address);

    event_loop
        .run(susyweb::confirm::send_transaction_with_confirmation(
            &side_transport,
            susyweb::types::TransactionRequest {
                from: user_address.into(),
                to: Some(side_contract_address.into()),
                gas: None,
                gas_price: None,
                value: None,
                data: Some(payload.into()),
                condition: None,
                nonce: None,
            },
            Duration::from_secs(1),
            0,
        ))
        .unwrap();

    println!("\nSending message to side complete. Give it plenty of time to get mined and relayed\n");
    thread::sleep(Duration::from_millis(15000));


    //dwd

    //let main_susyweb = susyweb::SusyWeb::new(&main_transport);
    //let code_future = main_susyweb.sof().code(main_recipient_address.into(), None);
    //let code = event_loop.run(code_future).unwrap();
    //println!("code: {:?}", code);

    // TODO: remove
    //bridge1.kill().unwrap();

    // wait for bridge to shut down
    //thread::sleep(Duration::from_millis(1000));
    //susy_main.kill().unwrap();
    //susy_side.kill().unwrap();

    //assert!(false);

    let (payload, decoder) = bridge_contracts::test::functions::last_data::call();

    let response = event_loop
        .run(AsyncCall::new(
            &main_transport,
            main_recipient_address.into(),
            TIMEOUT,
            payload,
            decoder,
        ))
        .unwrap();

    assert_eq!(
        response,
        data_to_relay_to_main,
        "data was not relayed properly to the main chain"
    );

    bridge1.kill().unwrap();

    // wait for bridge to shut down
    thread::sleep(Duration::from_millis(1000));

    susy_main.kill().unwrap();
    susy_side.kill().unwrap();
}
