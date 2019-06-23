# deployment and run guide

[susytech/susy-bridge](https://octonion.institute/susytech/susy-bridge)

this guide assumes that you are one of the authorities of
a PoA chain `side` and want to use the bridge to connect
`side` to another chain `main`.

since all bridge authorities use the same contracts on `side` and `main`
one authority has to go ahead and deploy them.

let's call this the **deploying authority**.

if the process is done correctly the other non-deploying authorities don't have to trust
the deploying authority.

upfront you must know the addresses of all authorities (`authorities`)
es well as the number of `required_signatures`

## initial deployment steps for any authority (deploying and non-deploying)

assuming you are authority with `authority_address`.

[build and install the bridge](https://octonion.institute/susytech/susy-bridge/#build)

install susy.
we tested it with [susy 2.0.4](https://octonion.institute/susytech/susy/releases/tag/v2.0.4) with Byzantium fork
enabled, though it should work with the latest stable release.

install polynomial compiler
we tested it with [polc 0.5.2](https://github.com/susy-lang/polynomial/releases/tag/v0.5.2)

start a susy node that connects to `main` chain, has `authority_address` unlocked
and http enabled at `main.http`. TODO add instructions. please refer to
the susy documentation for now.

start a susy node that connects to `side` chain, has `authority_address` unlocked
and http enabled at `side.http`. TODO add instructions. please refer to
the susy documentation for now.

### configure the bridge

copy [integration-tests/bridge_config.toml](https://octonion.institute/susytech/susy-bridge/blob/master/integration-tests/bridge_config.toml)
to a local `bridge_config.toml`.

within `bridge_config.toml` resolve/fill-in all the `ACTION REQUIRED`s.

for help refer to the comments, [the config option documentation](https://octonion.institute/susytech/susy-bridge/#configuration),
or [![Join the chat at https://gitter.im/susytech/susy-bridge](https://badges.gitter.im/susytech/susy-bridge.svg)](https://gitter.im/susytech/susy-bridge?utm_source=badge&utm_medium=badge&utm_campaign=pr-badge&utm_content=badge)

[if you're the deploying authority continue here](#further-deployment-steps-for-deploying-authority)

[if you're a non-deploying authority continue here](#further-run-steps)

## further deployment steps for deploying authority

start the bridge-deploy by executing:

```
env RUST_LOG=info susy-bridge-deploy --config bridge_config.toml --database bridge.db
```

it should eventually print something like this:

```
INFO:bridge: Deployed new bridge contracts
INFO:bridge:
main_contract_address = "0xebd3944af37ccc6b67ff61239ac4fef229c8f69f"
side_contract_address = "0xebd3944af37ccc6b67ff61239ac4fef229c8f69f"
main_deployed_at_block = 1
side_deployed_at_block = 1
last_main_to_side_sign_at_block = 1
last_side_to_main_signatures_at_block = 1
last_side_to_main_sign_at_block = 1
```

**congratulations! the bridge has successfully deployed its contracts on both chains**

`bridge.db` should now look similar to this:

```
main_contract_address = "0xebd3944af37ccc6b67ff61239ac4fef229c8f69f"
side_contract_address = "0xebd3944af37ccc6b67ff61239ac4fef229c8f69f"
main_deployed_at_block = 1
side_deployed_at_block = 1
last_main_to_side_sign_at_block = 1
last_side_to_main_signatures_at_block = 1
last_side_to_main_sign_at_block = 1
```

(verify the contracts deployed to `main_contract_address` and
`side_contract_address` using
[https://sophyscan.io/verifyContract](https://sophyscan.io/verifyContract) so the other authorities
can verify that you did an honest deploy without having to trust you.)

give the `bridge.db` file to the other authorities.
for example by posting it as a gist.
the database file doesn't contain any sensitive information.

ask the other authorities to follow **this guide you're reading**.

proceed to the next step to run the bridge.

## further run steps

you MUST receive a `bridge.db` from the deploying authority.

it should look similar to this:

```
main_contract_address = "0xebd3944af37ccc6b67ff61239ac4fef229c8f69f"
side_contract_address = "0xebd3944af37ccc6b67ff61239ac4fef229c8f69f"
main_deployed_at_block = 1
side_deployed_at_block = 1
last_main_to_side_sign_at_block = 3
last_side_to_main_signatures_at_block = 4
last_side_to_main_sign_at_block = 4
```

(check that the contracts deployed to
`main_contract_address` and `side_contract_address` are
verified on [https://sophyscan.io](https://sophyscan.io) and that the source code matches
the code in the repo.)

start the bridge by executing:

```
env RUST_LOG=info bridge --config bridge_config.toml --database bridge.db
```

it should eventually print this line:

```
 INFO XXXX-XX-XXTXX:XX:XXZ: susy_bridge: Started polling logs
```

**congratulations! the bridge has successfully started and joined the other authorities**

ensure the process keeps running. else the bridge won't function.
(outside the scope of this guide, your devops team knows what to do).
