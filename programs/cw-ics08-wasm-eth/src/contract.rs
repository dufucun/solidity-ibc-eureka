//! This module contains the `CosmWasm` entrypoints for the 08-wasm smart contract

use cosmwasm_std::{entry_point, Binary, Deps, DepsMut, Env, MessageInfo, Response};

use crate::msg::{ExecuteMsg, InstantiateMsg, QueryMsg, SudoMsg};
use crate::{custom_query::EthereumCustomQuery, instantiate, msg::MigrateMsg, query};
use crate::{sudo, ContractError};

/// The version of the contracts state.
/// It is used to determine if the state needs to be migrated in the migrate entry point.
const STATE_VERSION: &str = env!("CARGO_PKG_VERSION");
const CONTRACT_NAME: &str = env!("CARGO_PKG_NAME");

/// The instantiate entry point for the CosmWasm contract.
/// # Errors
/// Will return an error if the client state or consensus state cannot be deserialized.
/// # Panics
/// Will panic if the client state latest height cannot be unwrapped
#[entry_point]
#[allow(clippy::needless_pass_by_value)]
pub fn instantiate(
    deps: DepsMut<EthereumCustomQuery>,
    _env: Env,
    _info: MessageInfo,
    msg: InstantiateMsg,
) -> Result<Response, ContractError> {
    cw2::set_contract_version(deps.storage, CONTRACT_NAME, STATE_VERSION)?;

    instantiate::client(deps.storage, msg)?;

    Ok(Response::default())
}

/// The sudo entry point for the CosmWasm contract.
/// It routes the message to the appropriate handler.
/// # Errors
/// Will return an error if the handler returns an error.
#[entry_point]
#[allow(clippy::needless_pass_by_value)]
pub fn sudo(
    deps: DepsMut<EthereumCustomQuery>,
    _env: Env,
    msg: SudoMsg,
) -> Result<Response, ContractError> {
    let result = match msg {
        SudoMsg::VerifyMembership(verify_membership_msg) => {
            sudo::verify_membership(deps.as_ref(), verify_membership_msg)?
        }
        SudoMsg::VerifyNonMembership(verify_non_membership_msg) => {
            sudo::verify_non_membership(deps.as_ref(), verify_non_membership_msg)?
        }
        SudoMsg::UpdateState(update_state_msg) => sudo::update_state(deps, update_state_msg)?,
        SudoMsg::UpdateStateOnMisbehaviour(misbehaviour_msg) => {
            sudo::misbehaviour(deps, misbehaviour_msg)?
        }
        SudoMsg::VerifyUpgradeAndUpdateState(_) => todo!(),
        SudoMsg::MigrateClientStore(_) => todo!(),
    };

    Ok(Response::default().set_data(result))
}

/// Execute entry point is not used in this contract.
#[entry_point]
#[allow(clippy::needless_pass_by_value, clippy::missing_errors_doc)]
pub fn execute(
    _deps: DepsMut,
    _env: Env,
    _info: MessageInfo,
    _msg: ExecuteMsg,
) -> Result<Response, ContractError> {
    unimplemented!()
}

/// The query entry point for the CosmWasm contract.
/// It routes the message to the appropriate handler.
/// # Errors
/// Will return an error if the handler returns an error.
#[entry_point]
pub fn query(
    deps: Deps<EthereumCustomQuery>,
    env: Env,
    msg: QueryMsg,
) -> Result<Binary, ContractError> {
    match msg {
        QueryMsg::VerifyClientMessage(verify_client_message_msg) => {
            query::verify_client_message(deps, env, verify_client_message_msg)
        }
        QueryMsg::CheckForMisbehaviour(check_for_misbehaviour_msg) => {
            query::check_for_misbehaviour(deps, env, check_for_misbehaviour_msg)
        }
        QueryMsg::TimestampAtHeight(timestamp_at_height_msg) => {
            query::timestamp_at_height(deps, timestamp_at_height_msg)
        }
        QueryMsg::Status(_) => query::status(deps),
    }
}

/// The migrate entry point for the CosmWasm contract.
/// # Errors
/// Will return an errror if the state version is not newer than the current one.
#[entry_point]
#[allow(clippy::needless_pass_by_value)]
pub fn migrate(
    deps: DepsMut<EthereumCustomQuery>,
    _env: Env,
    msg: MigrateMsg,
) -> Result<Response, ContractError> {
    // Check if the state version is older than the current one and update it
    cw2::ensure_from_older_version(deps.storage, CONTRACT_NAME, STATE_VERSION)?;

    // Re-initialize the client if needed.
    if let Some(instantiate_msg) = msg.instantiate_msg {
        instantiate::client(deps.storage, instantiate_msg)?;
    }

    Ok(Response::default())
}

#[cfg(test)]
mod tests {
    mod instantiate_tests {
        use alloy_primitives::{Address, FixedBytes, B256, U256};
        use cosmwasm_std::{
            coins,
            testing::{message_info, mock_env},
            Storage,
        };
        use ethereum_light_client::{
            client_state::ClientState as EthClientState,
            consensus_state::ConsensusState as EthConsensusState,
        };
        use ethereum_types::consensus::fork::{Fork, ForkParameters};
        use ibc_proto::{
            google::protobuf::Any,
            ibc::lightclients::wasm::v1::{
                ClientState as WasmClientState, ConsensusState as WasmConsensusState,
            },
        };
        use prost::{Message, Name};

        use crate::{
            contract::instantiate,
            msg::InstantiateMsg,
            state::{consensus_db_key, HOST_CLIENT_STATE_KEY},
            test::mk_deps,
        };

        #[test]
        fn test_instantiate() {
            let mut deps = mk_deps();
            let creator = deps.api.addr_make("creator");
            let info = message_info(&creator, &coins(1, "uatom"));

            let client_state = EthClientState {
                chain_id: 0,
                genesis_validators_root: B256::from([0; 32]),
                min_sync_committee_participants: 0,
                genesis_time: 0,
                genesis_slot: 0,
                fork_parameters: ForkParameters {
                    genesis_fork_version: FixedBytes([0; 4]),
                    genesis_slot: 0,
                    altair: Fork {
                        version: FixedBytes([0; 4]),
                        epoch: 0,
                    },
                    bellatrix: Fork {
                        version: FixedBytes([0; 4]),
                        epoch: 0,
                    },
                    capella: Fork {
                        version: FixedBytes([0; 4]),
                        epoch: 0,
                    },
                    deneb: Fork {
                        version: FixedBytes([0; 4]),
                        epoch: 0,
                    },
                    electra: Fork {
                        version: FixedBytes([0; 4]),
                        epoch: 0,
                    },
                },
                seconds_per_slot: 10,
                slots_per_epoch: 8,
                epochs_per_sync_committee_period: 0,
                latest_slot: 42,
                latest_execution_block_number: 38,
                ibc_commitment_slot: U256::from(0),
                ibc_contract_address: Address::default(),
                is_frozen: false,
            };
            let client_state_bz: Vec<u8> = serde_json::to_vec(&client_state).unwrap();

            let consensus_state = EthConsensusState {
                slot: 42,
                state_root: B256::from([0; 32]),
                storage_root: B256::from([0; 32]),
                timestamp: 0,
                current_sync_committee: FixedBytes::<48>::from([0; 48]),
                next_sync_committee: None,
            };
            let consensus_state_bz: Vec<u8> = serde_json::to_vec(&consensus_state).unwrap();

            let msg = InstantiateMsg {
                client_state: client_state_bz.into(),
                consensus_state: consensus_state_bz.into(),
                checksum: b"also does not matter yet".into(),
            };

            let res = instantiate(deps.as_mut(), mock_env(), info, msg.clone()).unwrap();
            assert_eq!(0, res.messages.len());

            let actual_wasm_client_state_any_bz =
                deps.storage.get(HOST_CLIENT_STATE_KEY.as_bytes()).unwrap();
            let actual_wasm_client_state_any =
                Any::decode(actual_wasm_client_state_any_bz.as_slice()).unwrap();
            assert_eq!(
                WasmClientState::type_url(),
                actual_wasm_client_state_any.type_url
            );
            let actual_client_state =
                WasmClientState::decode(actual_wasm_client_state_any.value.as_slice()).unwrap();
            assert_eq!(msg.checksum, actual_client_state.checksum);
            assert_eq!(msg.client_state, actual_client_state.data);
            assert_eq!(
                0,
                actual_client_state.latest_height.unwrap().revision_number
            );
            assert_eq!(
                client_state.latest_slot,
                actual_client_state.latest_height.unwrap().revision_height
            );

            let actual_wasm_consensus_state_any_bz = deps
                .storage
                .get(consensus_db_key(consensus_state.slot).as_bytes())
                .unwrap();
            let actual_wasm_consensus_state_any =
                Any::decode(actual_wasm_consensus_state_any_bz.as_slice()).unwrap();
            assert_eq!(
                WasmConsensusState::type_url(),
                actual_wasm_consensus_state_any.type_url
            );
            let actual_consensus_state =
                WasmConsensusState::decode(actual_wasm_consensus_state_any.value.as_slice())
                    .unwrap();
            assert_eq!(msg.consensus_state, actual_consensus_state.data);
        }
    }

    mod integration_tests {
        use alloy_primitives::{Address, FixedBytes, B256, U256};
        use cosmwasm_std::{
            coins,
            testing::{message_info, mock_env},
            Binary, Storage, Timestamp,
        };
        use ethereum_light_client::{
            client_state::ClientState as EthClientState,
            consensus_state::ConsensusState as EthConsensusState,
            header::Header,
            test_utils::fixtures::{
                self, get_packet_proof, InitialState, RelayerMessages, StepsFixture,
            },
        };
        use ethereum_types::consensus::fork::{Fork, ForkParameters};
        use ibc_proto::{
            google::protobuf::Any,
            ibc::lightclients::wasm::v1::{ClientMessage, ClientState as WasmClientState},
        };
        use prost::Message;

        use crate::{
            contract::{instantiate, migrate, query, sudo},
            msg::{
                Height, InstantiateMsg, MerklePath, MigrateMsg, QueryMsg, SudoMsg, UpdateStateMsg,
                UpdateStateResult, VerifyClientMessageMsg, VerifyMembershipMsg,
            },
            state::HOST_CLIENT_STATE_KEY,
            test::mk_deps,
        };

        #[test]
        // This test runs throught the e2e test scenario defined in the interchaintest:
        // TestICS20TransferERC20TokenfromEthereumToCosmosAndBack_Groth16
        fn test_ics20_transfer_from_ethereum_to_cosmos_flow() {
            let mut deps = mk_deps();
            let creator = deps.api.addr_make("creator");
            let info = message_info(&creator, &coins(1, "uatom"));

            let fixture: StepsFixture =
                fixtures::load("TestICS20TransferERC20TokenfromEthereumToCosmosAndBack_Groth16");

            let initial_state: InitialState = fixture.get_data_at_step(0);

            let client_state = initial_state.client_state;

            let consensus_state = initial_state.consensus_state;

            let client_state_bz: Vec<u8> = serde_json::to_vec(&client_state).unwrap();
            let consensus_state_bz: Vec<u8> = serde_json::to_vec(&consensus_state).unwrap();

            let msg = crate::msg::InstantiateMsg {
                client_state: Binary::from(client_state_bz),
                consensus_state: Binary::from(consensus_state_bz),
                checksum: b"checksum".into(),
            };

            instantiate(deps.as_mut(), mock_env(), info, msg).unwrap();

            // At this point, the light clients are initialized and the client state is stored
            // In the flow, an ICS20 transfer has been initiated from Ethereum to Cosmos
            // Next up we want to prove the packet on the Cosmos chain, so we start by updating the
            // light client (which is two steps: verify client message and update state)

            // Verify client message
            let relayer_messages: RelayerMessages = fixture.get_data_at_step(1);
            let (update_client_msgs, recv_msgs, _) = relayer_messages.get_sdk_msgs();
            assert_eq!(1, update_client_msgs.len()); // just to make sure
            assert_eq!(1, recv_msgs.len()); // just to make sure
            let client_msgs = update_client_msgs
                .iter()
                .map(|msg| {
                    ClientMessage::decode(msg.client_message.clone().unwrap().value.as_slice())
                        .unwrap()
                })
                .map(|msg| msg.data)
                .collect::<Vec<_>>();

            let mut env = mock_env();

            for header_bz in client_msgs {
                let header: Header = serde_json::from_slice(&header_bz).unwrap();
                env.block.time = Timestamp::from_seconds(
                    header.consensus_update.attested_header.execution.timestamp + 1000,
                );

                let query_verify_client_msg =
                    QueryMsg::VerifyClientMessage(VerifyClientMessageMsg {
                        client_message: Binary::from(header_bz.clone()),
                    });
                query(deps.as_ref(), env.clone(), query_verify_client_msg).unwrap();

                // Update state
                let sudo_update_state_msg = SudoMsg::UpdateState(UpdateStateMsg {
                    client_message: Binary::from(header_bz),
                });
                let update_res = sudo(deps.as_mut(), env.clone(), sudo_update_state_msg).unwrap();
                let update_state_result: UpdateStateResult =
                    serde_json::from_slice(&update_res.data.unwrap())
                        .expect("update state result should be deserializable");
                assert_eq!(1, update_state_result.heights.len());
                assert_eq!(0, update_state_result.heights[0].revision_number);
                assert_eq!(
                    header.consensus_update.finalized_header.beacon.slot,
                    update_state_result.heights[0].revision_height
                );
            }

            // The client has now been updated, and we would submit the packet to the cosmos chain,
            // along with the proof of th packet commitment. IBC will call verify_membership.

            // Verify memebership
            let packet = recv_msgs[0].packet.clone().unwrap();
            let storage_proof = recv_msgs[0].proof_commitment.clone();
            let (path, value) = get_packet_proof(packet);

            let query_verify_membership_msg = SudoMsg::VerifyMembership(VerifyMembershipMsg {
                height: Height {
                    revision_number: 0,
                    revision_height: recv_msgs[0].proof_height.unwrap().revision_height,
                },
                delay_time_period: 0,
                delay_block_period: 0,
                proof: Binary::from(storage_proof),
                merkle_path: MerklePath {
                    key_path: vec![Binary::from(path)],
                },
                value: Binary::from(value),
            });
            sudo(deps.as_mut(), env, query_verify_membership_msg).unwrap();
        }

        #[test]
        fn test_update_with_period_change() {
            let mut deps = mk_deps();
            let creator = deps.api.addr_make("creator");
            let info = message_info(&creator, &coins(1, "uatom"));

            let fixture: StepsFixture = fixtures::load("TestMultiPeriodClientUpdateToCosmos");

            let initial_state: InitialState = fixture.get_data_at_step(0);

            let client_state = initial_state.client_state;

            let consensus_state = initial_state.consensus_state;

            let client_state_bz: Vec<u8> = serde_json::to_vec(&client_state).unwrap();
            let consensus_state_bz: Vec<u8> = serde_json::to_vec(&consensus_state).unwrap();

            let msg = crate::msg::InstantiateMsg {
                client_state: Binary::from(client_state_bz),
                consensus_state: Binary::from(consensus_state_bz),
                checksum: b"checksum".into(),
            };

            instantiate(deps.as_mut(), mock_env(), info, msg).unwrap();

            // At this point, the light clients are initialized and the client state is stored
            // In the flow, an ICS20 transfer has been initiated from Ethereum to Cosmos
            // Next up we want to prove the packet on the Cosmos chain, so we start by updating the
            // light client (which is two steps: verify client message and update state)

            // Verify client message
            let relayer_messages: RelayerMessages = fixture.get_data_at_step(1);
            let (update_client_msgs, recv_msgs, _) = relayer_messages.get_sdk_msgs();
            assert!(update_client_msgs.len() >= 2); // just to make sure
            assert_eq!(1, recv_msgs.len()); // just to make sure
            let client_msgs = update_client_msgs
                .iter()
                .map(|msg| {
                    ClientMessage::decode(msg.client_message.clone().unwrap().value.as_slice())
                        .unwrap()
                })
                .map(|msg| msg.data)
                .collect::<Vec<_>>();

            let mut env = mock_env();

            for header_bz in client_msgs {
                let header: Header = serde_json::from_slice(&header_bz).unwrap();
                env.block.time = Timestamp::from_seconds(
                    header.consensus_update.attested_header.execution.timestamp + 1000,
                );

                let query_verify_client_msg =
                    QueryMsg::VerifyClientMessage(VerifyClientMessageMsg {
                        client_message: Binary::from(header_bz.clone()),
                    });
                query(deps.as_ref(), env.clone(), query_verify_client_msg).unwrap();

                // Update state
                let sudo_update_state_msg = SudoMsg::UpdateState(UpdateStateMsg {
                    client_message: Binary::from(header_bz),
                });
                let update_res = sudo(deps.as_mut(), env.clone(), sudo_update_state_msg).unwrap();
                let update_state_result: UpdateStateResult =
                    serde_json::from_slice(&update_res.data.unwrap())
                        .expect("update state result should be deserializable");
                assert_eq!(1, update_state_result.heights.len());
                assert_eq!(0, update_state_result.heights[0].revision_number);
                assert_eq!(
                    header.consensus_update.finalized_header.beacon.slot,
                    update_state_result.heights[0].revision_height
                );
            }

            // The client has now been updated, and we would submit the packet to the cosmos chain,
            // along with the proof of th packet commitment. IBC will call verify_membership.

            // Verify memebership
            let packet = recv_msgs[0].packet.clone().unwrap();
            let storage_proof = recv_msgs[0].proof_commitment.clone();
            let (path, value) = get_packet_proof(packet);

            let query_verify_membership_msg = SudoMsg::VerifyMembership(VerifyMembershipMsg {
                height: Height {
                    revision_number: 0,
                    revision_height: recv_msgs[0].proof_height.unwrap().revision_height,
                },
                delay_time_period: 0,
                delay_block_period: 0,
                proof: Binary::from(storage_proof),
                merkle_path: MerklePath {
                    key_path: vec![Binary::from(path)],
                },
                value: Binary::from(value),
            });
            sudo(deps.as_mut(), env, query_verify_membership_msg).unwrap();
        }

        // TODO: Remove this test (and the fixture) after Deneb support is removed (#440)
        #[test]
        #[allow(clippy::too_many_lines)]
        fn test_electra_fork() {
            let mut deps = mk_deps();
            let creator = deps.api.addr_make("creator");
            let info = message_info(&creator, &coins(1, "uatom"));

            let fixture: StepsFixture = fixtures::load("Test_Electra_Fork");

            // Initial state is at Deneb fork
            let initial_state: InitialState = fixture.get_data_at_step(0);

            let client_state = initial_state.client_state;

            let consensus_state = initial_state.consensus_state;

            let client_state_bz: Vec<u8> = serde_json::to_vec(&client_state).unwrap();
            let consensus_state_bz: Vec<u8> = serde_json::to_vec(&consensus_state).unwrap();

            let msg = crate::msg::InstantiateMsg {
                client_state: Binary::from(client_state_bz),
                consensus_state: Binary::from(consensus_state_bz),
                checksum: b"checksum".into(),
            };

            instantiate(deps.as_mut(), mock_env(), info, msg).unwrap();

            // At this point, we're at Deneb, and the light client is initialized
            // Now we will get a client update that is still at Deneb, and a transfer

            // We get the relayer messages
            let relayer_messages: RelayerMessages = fixture.get_data_at_step(1);
            let (update_client_msgs, recv_msgs, _) = relayer_messages.get_sdk_msgs();
            assert!(!update_client_msgs.is_empty()); // just to make sure
            assert_eq!(1, recv_msgs.len()); // just to make sure
            let client_msgs = update_client_msgs
                .iter()
                .map(|msg| {
                    ClientMessage::decode(msg.client_message.clone().unwrap().value.as_slice())
                        .unwrap()
                })
                .map(|msg| msg.data)
                .collect::<Vec<_>>();

            let mut env = mock_env();

            for header_bz in client_msgs {
                let header: Header = serde_json::from_slice(&header_bz).unwrap();
                env.block.time = Timestamp::from_seconds(
                    header.consensus_update.attested_header.execution.timestamp + 1000,
                );

                let query_verify_client_msg =
                    QueryMsg::VerifyClientMessage(VerifyClientMessageMsg {
                        client_message: Binary::from(header_bz.clone()),
                    });
                query(deps.as_ref(), env.clone(), query_verify_client_msg).unwrap();

                // Update state
                let sudo_update_state_msg = SudoMsg::UpdateState(UpdateStateMsg {
                    client_message: Binary::from(header_bz),
                });
                let update_res = sudo(deps.as_mut(), env.clone(), sudo_update_state_msg).unwrap();
                let update_state_result: UpdateStateResult =
                    serde_json::from_slice(&update_res.data.unwrap())
                        .expect("update state result should be deserializable");
                assert_eq!(1, update_state_result.heights.len());
                assert_eq!(0, update_state_result.heights[0].revision_number);
                assert_eq!(
                    header.consensus_update.finalized_header.beacon.slot,
                    update_state_result.heights[0].revision_height
                );
            }

            // The client has now been updated, and we would submit the packet to the cosmos chain,
            // along with the proof of th packet commitment. IBC will call verify_membership.

            // Verify memebership
            let packet = recv_msgs[0].packet.clone().unwrap();
            let storage_proof = recv_msgs[0].proof_commitment.clone();
            let (path, value) = get_packet_proof(packet);

            let query_verify_membership_msg = SudoMsg::VerifyMembership(VerifyMembershipMsg {
                height: Height {
                    revision_number: 0,
                    revision_height: recv_msgs[0].proof_height.unwrap().revision_height,
                },
                delay_time_period: 0,
                delay_block_period: 0,
                proof: Binary::from(storage_proof),
                merkle_path: MerklePath {
                    key_path: vec![Binary::from(path)],
                },
                value: Binary::from(value),
            });
            sudo(deps.as_mut(), env, query_verify_membership_msg).unwrap();

            // The chain itself has forked to Electra, but the client is still at Deneb
            // The next update essentially takes ut to Electra, and we process packets from the
            // Electra fork

            // We get the relayer messages
            let relayer_messages: RelayerMessages = fixture.get_data_at_step(2);
            let (update_client_msgs, recv_msgs, _) = relayer_messages.get_sdk_msgs();
            assert!(!update_client_msgs.is_empty()); // just to make sure
            assert_eq!(1, recv_msgs.len()); // just to make sure
            let client_msgs = update_client_msgs
                .iter()
                .map(|msg| {
                    ClientMessage::decode(msg.client_message.clone().unwrap().value.as_slice())
                        .unwrap()
                })
                .map(|msg| msg.data)
                .collect::<Vec<_>>();

            let mut env = mock_env();

            for header_bz in client_msgs {
                let header: Header = serde_json::from_slice(&header_bz).unwrap();
                env.block.time = Timestamp::from_seconds(
                    header.consensus_update.attested_header.execution.timestamp + 1000,
                );

                let query_verify_client_msg =
                    QueryMsg::VerifyClientMessage(VerifyClientMessageMsg {
                        client_message: Binary::from(header_bz.clone()),
                    });
                query(deps.as_ref(), env.clone(), query_verify_client_msg).unwrap();

                // Update state
                let sudo_update_state_msg = SudoMsg::UpdateState(UpdateStateMsg {
                    client_message: Binary::from(header_bz),
                });
                let update_res = sudo(deps.as_mut(), env.clone(), sudo_update_state_msg).unwrap();
                let update_state_result: UpdateStateResult =
                    serde_json::from_slice(&update_res.data.unwrap())
                        .expect("update state result should be deserializable");
                assert_eq!(1, update_state_result.heights.len());
                assert_eq!(0, update_state_result.heights[0].revision_number);
                assert_eq!(
                    header.consensus_update.finalized_header.beacon.slot,
                    update_state_result.heights[0].revision_height
                );
            }

            // The client has now been updated to Electra, and we would submit the packet to the cosmos chain,
            // along with the proof of th packet commitment. IBC will call verify_membership.

            // Verify memebership
            let packet = recv_msgs[0].packet.clone().unwrap();
            let storage_proof = recv_msgs[0].proof_commitment.clone();
            let (path, value) = get_packet_proof(packet);

            let query_verify_membership_msg = SudoMsg::VerifyMembership(VerifyMembershipMsg {
                height: Height {
                    revision_number: 0,
                    revision_height: recv_msgs[0].proof_height.unwrap().revision_height,
                },
                delay_time_period: 0,
                delay_block_period: 0,
                proof: Binary::from(storage_proof),
                merkle_path: MerklePath {
                    key_path: vec![Binary::from(path)],
                },
                value: Binary::from(value),
            });
            sudo(deps.as_mut(), env, query_verify_membership_msg).unwrap();
        }

        #[test]
        fn test_migrate_with_same_state_version() {
            let mut deps = mk_deps();
            let creator = deps.api.addr_make("creator");
            let info = message_info(&creator, &coins(1, "uatom"));

            let fixture: StepsFixture =
                fixtures::load("TestICS20TransferERC20TokenfromEthereumToCosmosAndBack_Groth16");

            let initial_state: InitialState = fixture.get_data_at_step(0);

            let client_state = initial_state.client_state;

            let consensus_state = initial_state.consensus_state;

            let client_state_bz: Vec<u8> = serde_json::to_vec(&client_state).unwrap();
            let consensus_state_bz: Vec<u8> = serde_json::to_vec(&consensus_state).unwrap();

            let msg = crate::msg::InstantiateMsg {
                client_state: Binary::from(client_state_bz),
                consensus_state: Binary::from(consensus_state_bz),
                checksum: b"checksum".into(),
            };

            instantiate(deps.as_mut(), mock_env(), info, msg).unwrap();

            // Migrate without any changes (i.e. same state version)
            migrate(
                deps.as_mut(),
                mock_env(),
                crate::msg::MigrateMsg {
                    instantiate_msg: None,
                },
            )
            .unwrap();
        }

        #[test]
        fn test_migrate_with_instantiate() {
            let mut deps = mk_deps();
            let creator = deps.api.addr_make("creator");
            let info = message_info(&creator, &coins(1, "uatom"));

            let client_state = EthClientState {
                chain_id: 0,
                genesis_validators_root: B256::from([0; 32]),
                min_sync_committee_participants: 0,
                genesis_time: 0,
                genesis_slot: 0,
                fork_parameters: ForkParameters {
                    genesis_fork_version: FixedBytes([0; 4]),
                    genesis_slot: 0,
                    altair: Fork {
                        version: FixedBytes([0; 4]),
                        epoch: 0,
                    },
                    bellatrix: Fork {
                        version: FixedBytes([0; 4]),
                        epoch: 0,
                    },
                    capella: Fork {
                        version: FixedBytes([0; 4]),
                        epoch: 0,
                    },
                    deneb: Fork {
                        version: FixedBytes([0; 4]),
                        epoch: 0,
                    },
                    electra: Fork {
                        version: FixedBytes([0; 4]),
                        epoch: 0,
                    },
                },
                seconds_per_slot: 10,
                slots_per_epoch: 8,
                epochs_per_sync_committee_period: 0,
                latest_slot: 42,
                latest_execution_block_number: 38,
                ibc_commitment_slot: U256::from(0),
                ibc_contract_address: Address::default(),
                is_frozen: false,
            };
            let client_state_bz: Vec<u8> = serde_json::to_vec(&client_state).unwrap();

            let consensus_state = EthConsensusState {
                slot: 42,
                state_root: B256::from([0; 32]),
                storage_root: B256::from([0; 32]),
                timestamp: 0,
                current_sync_committee: FixedBytes::<48>::from([0; 48]),
                next_sync_committee: None,
            };
            let consensus_state_bz: Vec<u8> = serde_json::to_vec(&consensus_state).unwrap();

            let msg = InstantiateMsg {
                client_state: client_state_bz.into(),
                consensus_state: consensus_state_bz.into(),
                checksum: b"does not matter yet".into(),
            };

            let res = instantiate(deps.as_mut(), mock_env(), info, msg).unwrap();
            assert_eq!(0, res.messages.len());

            let fixture: StepsFixture = fixtures::load("Test_Electra_Fork");

            // Initial state is at Electra fork
            let initial_state: InitialState = fixture.get_data_at_step(0);

            let client_state_fixture = initial_state.client_state;

            let consensus_state_fixture = initial_state.consensus_state;

            let client_state_fixture_bz: Vec<u8> =
                serde_json::to_vec(&client_state_fixture).unwrap();
            let consensus_state_fixture_bz: Vec<u8> =
                serde_json::to_vec(&consensus_state_fixture).unwrap();

            let msg = InstantiateMsg {
                client_state: Binary::from(client_state_fixture_bz),
                consensus_state: Binary::from(consensus_state_fixture_bz),
                checksum: b"checksum".into(),
            };

            let migrate_msg = MigrateMsg {
                instantiate_msg: Some(msg.clone()),
            };

            // Migrate without any changes (i.e. same state version)
            migrate(deps.as_mut(), mock_env(), migrate_msg).unwrap();

            let actual_wasm_client_state_any_bz =
                deps.storage.get(HOST_CLIENT_STATE_KEY.as_bytes()).unwrap();
            let actual_wasm_client_state_any =
                Any::decode(actual_wasm_client_state_any_bz.as_slice()).unwrap();
            let wasm_client_state =
                WasmClientState::decode(actual_wasm_client_state_any.value.as_slice()).unwrap();
            assert_eq!(msg.checksum, wasm_client_state.checksum);
            assert_ne!(
                wasm_client_state.latest_height.unwrap().revision_height,
                client_state.latest_slot
            );
        }
    }
}
