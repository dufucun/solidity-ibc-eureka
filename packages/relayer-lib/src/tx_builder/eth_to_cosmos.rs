//! This module defines [`TxBuilder`] which is responsible for building transactions to be sent to
//! the Cosmos SDK chain from events received from Ethereum.

use std::time::Duration;

use alloy::{primitives::Address, providers::Provider};
use anyhow::Result;
use ethereum_apis::{beacon_api::client::BeaconApiClient, eth_api::client::EthApiClient};
use ethereum_light_client::header::{AccountUpdate, ActiveSyncCommittee};
use ethereum_light_client::{client_state::ClientState, header::Header};
use ethereum_types::consensus::light_client_header::{
    LightClientFinalityUpdate, LightClientUpdate,
};
use ethereum_types::consensus::sync_committee::SyncCommittee;
use ethereum_types::execution::account_proof::AccountProof;
use ibc_eureka_solidity_types::ics26::router::routerInstance;
use ibc_eureka_utils::rpc::TendermintRpcExt;
use ibc_proto_eureka::cosmos::tx::v1beta1::TxBody;
use ibc_proto_eureka::google::protobuf::Any;
use ibc_proto_eureka::ibc::core::client::v1::{Height, MsgUpdateClient};
use ibc_proto_eureka::ibc::lightclients::wasm::v1::ClientMessage;
use ibc_proto_eureka::ibc::lightclients::wasm::v1::ClientState as WasmClientState;
use prost::Message;
use tendermint_rpc::{Client, HttpClient};

use crate::utils::{cosmos, wait_for_condition};
use crate::{
    chain::{CosmosSdk, EthEureka},
    events::EurekaEventWithHeight,
};

use super::r#trait::TxBuilderService;

/// The `TxBuilder` produces txs to [`CosmosSdk`] based on events from [`EthEureka`].
pub struct TxBuilder<P>
where
    P: Provider + Clone,
{
    /// The ETH API client.
    pub eth_client: EthApiClient<P>,
    /// The Beacon API client.
    pub beacon_api_client: BeaconApiClient,
    /// The IBC Eureka router instance.
    pub ics26_router: routerInstance<(), P>,
    /// The HTTP client for the Cosmos SDK.
    pub tm_client: HttpClient,
    /// The signer address for the Cosmos messages.
    pub signer_address: String,
}

/// The `MockTxBuilder` produces txs to [`CosmosSdk`] based on events from [`EthEureka`]
/// for testing purposes.
pub struct MockTxBuilder<P: Provider + Clone> {
    /// The ETH API client.
    pub eth_client: EthApiClient<P>,
    /// The IBC Eureka router instance.
    pub ics26_router: routerInstance<(), P>,
    /// The signer address for the Cosmos messages.
    pub signer_address: String,
}

impl<P> TxBuilder<P>
where
    P: Provider + Clone,
{
    /// Create a new [`TxBuilder`] instance.
    pub fn new(
        ics26_address: Address,
        provider: P,
        beacon_api_url: String,
        tm_client: HttpClient,
        signer_address: String,
    ) -> Self {
        Self {
            eth_client: EthApiClient::new(provider.clone()),
            beacon_api_client: BeaconApiClient::new(beacon_api_url),
            ics26_router: routerInstance::new(ics26_address, provider),
            tm_client,
            signer_address,
        }
    }

    /// Fetch the Ethereum client state from the light client on cosmos.
    /// # Errors
    /// Returns an error if the client state cannot be fetched or decoded.
    pub async fn ethereum_client_state(&self, client_id: String) -> Result<ClientState> {
        let wasm_client_state_any = self.tm_client.client_state(client_id).await?;
        let wasm_client_state = WasmClientState::decode(wasm_client_state_any.value.as_slice())?;
        Ok(serde_json::from_slice(&wasm_client_state.data)?)
    }

    async fn get_sync_commitee_for_finalized_slot(
        &self,
        finalized_slot: u64,
    ) -> Result<SyncCommittee> {
        let block_root = self
            .beacon_api_client
            .beacon_block_root(&format!("{finalized_slot}"))
            .await?;
        let light_client_bootstrap = self
            .beacon_api_client
            .light_client_bootstrap(&block_root)
            .await?
            .data;
        Ok(light_client_bootstrap.current_sync_committee)
    }

    /// Fetches light client updates from the Beacon API for synchronizing between the trusted and target periods.
    ///
    /// This function calculates the sync committee periods for both the trusted state and the finality update,
    /// then retrieves all light client updates needed to advance the light client from the trusted period
    /// to the target period. These updates contain validator signatures and sync committee data needed
    /// to verify the consensus transition.
    async fn get_light_client_updates(
        &self,
        client_state: &ClientState,
        finality_update: LightClientFinalityUpdate,
    ) -> Result<Vec<LightClientUpdate>> {
        let trusted_period =
            client_state.compute_sync_committee_period_at_slot(client_state.latest_slot);

        let target_period = client_state
            .compute_sync_committee_period_at_slot(finality_update.finalized_header.beacon.slot);

        tracing::debug!(
            "Getting light client updates from period {} to {}",
            trusted_period,
            target_period - trusted_period + 1
        );
        Ok(self
            .beacon_api_client
            .light_client_updates(trusted_period, target_period - trusted_period + 1)
            .await?
            .into_iter()
            .map(|resp| resp.data)
            .collect::<Vec<_>>())
    }

    async fn wait_for_light_client_readiness(&self, target_block_number: u64) -> Result<()> {
        // Wait until we find a finality update that meets our criteria and capture it
        // This way we avoid making an extra call at the end
        wait_for_condition(
            Duration::from_secs(45 * 60),
            Duration::from_secs(10),
            || async {
                tracing::debug!(
                    "Waiting for finality beyond target block number: {}",
                    target_block_number
                );

                let finality_update = self.beacon_api_client.finality_update().await?.data;
                if finality_update.finalized_header.execution.block_number < target_block_number {
                    tracing::info!(
                        "Waiting for finality: current finality execution block number: {}, Target execution block number: {}",
                        finality_update.finalized_header.execution.block_number,
                        target_block_number
                    );
                    return Ok(false);
                }

                // Return the update itself when the condition is met
                tracing::info!(
                    "Finality update found at execution block number: {}",
                    finality_update.finalized_header.execution.block_number
                );
                Ok(true)
            },
        )
        .await?;

        Ok(())
    }

    async fn light_client_update_to_header(
        &self,
        ethereum_client_state: &ClientState,
        active_sync_committee: ActiveSyncCommittee,
        update: LightClientUpdate,
    ) -> Result<Header> {
        tracing::debug!(
            "Processing light client update for finalized slot {} ",
            update.finalized_header.beacon.slot,
        );

        let block_hex = format!("0x{:x}", update.finalized_header.execution.block_number);
        let ibc_contract_address: String = ethereum_client_state.ibc_contract_address.to_string();

        tracing::debug!("Getting account proof for execution block {}", block_hex);
        let proof = self
            .eth_client
            .get_proof(&ibc_contract_address, vec![], block_hex)
            .await?;

        let account_update = AccountUpdate {
            account_proof: AccountProof {
                proof: proof.account_proof,
                storage_root: proof.storage_hash,
            },
        };

        Ok(Header {
            active_sync_committee,
            account_update,
            consensus_update: update,
        })
    }

    #[tracing::instrument(skip_all)]
    async fn get_update_headers(&self, ethereum_client_state: &ClientState) -> Result<Vec<Header>> {
        let finality_update = self.beacon_api_client.finality_update().await?.data;

        let mut headers = vec![];

        let light_client_updates = self
            .get_light_client_updates(ethereum_client_state, finality_update.clone())
            .await?;

        let mut latest_trusted_slot = ethereum_client_state.latest_slot;
        let mut latest_period =
            ethereum_client_state.compute_sync_committee_period_at_slot(latest_trusted_slot);
        tracing::debug!("Latest trusted sync committee period: {}", latest_period);

        for update in &light_client_updates {
            tracing::debug!(
                "Processing light client update for finalized slot {} with trusted slot {}",
                update.finalized_header.beacon.slot,
                latest_trusted_slot
            );

            if update.finalized_header.beacon.slot <= latest_trusted_slot {
                tracing::debug!(
                    "Skipping unnecessary update for slot {}",
                    update.finalized_header.beacon.slot
                );
                continue;
            }

            // TODO: Not sure
            let update_period = ethereum_client_state
                .compute_sync_committee_period_at_slot(update.finalized_header.beacon.slot);
            if update_period == latest_period {
                tracing::debug!(
                    "Skipping header with same sync committee period for slot {}",
                    update.finalized_header.beacon.slot
                );
                continue;
            }

            let previous_next_sync_committee = self
                .get_sync_commitee_for_finalized_slot(update.finalized_header.beacon.slot)
                .await?;

            let active_sync_committee = ActiveSyncCommittee::Next(previous_next_sync_committee);
            let header = self
                .light_client_update_to_header(
                    ethereum_client_state,
                    active_sync_committee.clone(),
                    update.clone(),
                )
                .await?;
            tracing::debug!(
                "Added header for slot from light client updates {}
                Header: {:?}",
                update.finalized_header.beacon.slot,
                header
            );
            headers.push(header);
            latest_period = update_period;
            latest_trusted_slot = update.finalized_header.beacon.slot;
        }

        // If the latest header is earlier than the finality update, we need to add a header for the finality update.
        if headers.last().is_none_or(|last_header| {
            last_header.consensus_update.finalized_header.beacon.slot
                < finality_update.finalized_header.beacon.slot
        }) {
            let finality_update_sync_committee = self
                .get_sync_commitee_for_finalized_slot(finality_update.attested_header.beacon.slot)
                .await?;
            // TODO: Add asserts to make sure they are in the correct period
            let active_sync_committee =
                ActiveSyncCommittee::Current(finality_update_sync_committee.clone());

            let header = self
                .light_client_update_to_header(
                    ethereum_client_state,
                    active_sync_committee.clone(),
                    finality_update.clone().into(),
                )
                .await?;
            tracing::debug!(
                "Added header for slot from finality update {}: {}",
                finality_update.finalized_header.beacon.slot,
                serde_json::to_string(&header)?
            );
            headers.push(header);
        }

        Ok(headers)
    }
}

#[async_trait::async_trait]
impl<P> TxBuilderService<EthEureka, CosmosSdk> for TxBuilder<P>
where
    P: Provider + Clone,
{
    #[tracing::instrument(skip_all)]
    async fn relay_events(
        &self,
        src_events: Vec<EurekaEventWithHeight>,
        dest_events: Vec<EurekaEventWithHeight>,
        src_client_id: String,
        dst_client_id: String,
        src_packet_seqs: Vec<u64>,
        dst_packet_seqs: Vec<u64>,
    ) -> Result<Vec<u8>> {
        let latest_block_number = self.eth_client.get_block_number().await?;
        let minimum_block_number = if dest_events.is_empty() {
            let latest_block_from_events = src_events.iter().filter_map(|e| e.block_number).max();
            latest_block_from_events.unwrap_or(latest_block_number)
        } else {
            // If we have destination events (e.g. timeout), use the latest block number
            latest_block_number
        };

        let target_height = Height {
            revision_number: 0,
            revision_height: minimum_block_number,
        };

        let now_since_unix = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH)?;

        let mut timeout_msgs = cosmos::target_events_to_timeout_msgs(
            dest_events,
            &src_client_id,
            &dst_client_id,
            &dst_packet_seqs,
            &target_height,
            &self.signer_address,
            now_since_unix.as_secs(),
        );

        let (mut recv_msgs, mut ack_msgs) = cosmos::src_events_to_recv_and_ack_msgs(
            src_events,
            &src_client_id,
            &dst_client_id,
            &src_packet_seqs,
            &dst_packet_seqs,
            &target_height,
            &self.signer_address,
            now_since_unix.as_secs(),
        );

        let mut ethereum_client_state = self.ethereum_client_state(dst_client_id.clone()).await?;

        tracing::info!(
            "Relaying events from Ethereum to Cosmos for client {}, target block number: {}, client state latest slot: {}",
            dst_client_id,
            minimum_block_number,
            ethereum_client_state.latest_slot,
        );

        // get updates if necessary
        let headers = if minimum_block_number > ethereum_client_state.latest_execution_block_number
        {
            self.wait_for_light_client_readiness(minimum_block_number)
                .await?;
            // Update the client state and consensus state, in case they have changed while we were waiting
            ethereum_client_state = self.ethereum_client_state(dst_client_id.clone()).await?;
            self.get_update_headers(&ethereum_client_state).await?
        } else {
            vec![]
        };

        let proof_slot = headers
            .last()
            .map_or(ethereum_client_state.latest_slot, |h| {
                h.consensus_update.finalized_header.beacon.slot
            });

        cosmos::inject_ethereum_proofs(
            &mut recv_msgs,
            &mut ack_msgs,
            &mut timeout_msgs,
            &self.eth_client,
            &self.beacon_api_client,
            &ethereum_client_state.ibc_contract_address.to_string(),
            ethereum_client_state.ibc_commitment_slot,
            proof_slot,
        )
        .await?;

        let update_msgs = headers
            .iter()
            .map(|header| -> Result<MsgUpdateClient> {
                let header_bz = serde_json::to_vec(&header)?;
                let client_msg = Any::from_msg(&ClientMessage { data: header_bz })?;
                Ok(MsgUpdateClient {
                    client_id: dst_client_id.clone(),
                    client_message: Some(client_msg),
                    signer: self.signer_address.clone(),
                })
            })
            .collect::<Result<Vec<_>, _>>()?;

        let all_msgs = update_msgs
            .into_iter()
            .map(|m| Any::from_msg(&m))
            .chain(timeout_msgs.iter().map(Any::from_msg))
            .chain(recv_msgs.iter().map(Any::from_msg))
            .chain(ack_msgs.iter().map(Any::from_msg))
            .collect::<Result<Vec<_>, _>>()?;

        let tx_body = TxBody {
            messages: all_msgs,
            ..Default::default()
        };

        let latest_signature_slot = headers.last().map(|h| h.consensus_update.signature_slot);

        // Final check to make sure the target chain's calculated slot is greater than our latest
        // update's signature slot
        wait_for_condition(
            Duration::from_secs(15 * 60),
            Duration::from_secs(10),
            || async {
                if headers.is_empty() {
                    return Ok(true);
                }

                let latests_tm_block = self.tm_client.latest_block().await?;
                let latest_onchain_timestamp = latests_tm_block.block.header.time.unix_timestamp();
                let calculated_slot = ethereum_client_state
                    .compute_slot_at_timestamp(latest_onchain_timestamp.try_into().unwrap())
                    .unwrap();
                tracing::debug!(
                    "Waiting for target chain to catch up to slot {}",
                    calculated_slot
                );
                Ok(calculated_slot > latest_signature_slot.unwrap())
            },
        )
        .await?;

        let initial_period = ethereum_client_state
            .compute_sync_committee_period_at_slot(ethereum_client_state.latest_slot);
        let latest_period = ethereum_client_state.compute_sync_committee_period_at_slot(proof_slot);
        tracing::info!(
            "Update client summary: 
                recv events processed: #{}, 
                ack events processed: #{}, 
                timeout events processed: #{}, 
                initial slot: {}, 
                latest trusted slot (after updates): {}, 
                initial period: {}, 
                latest period: {}, 
                number of headers: #{}",
            recv_msgs.len(),
            ack_msgs.len(),
            timeout_msgs.len(),
            ethereum_client_state.latest_slot,
            proof_slot,
            initial_period,
            latest_period,
            headers.len()
        );

        Ok(tx_body.encode_to_vec())
    }
}

impl<P: Provider + Clone> MockTxBuilder<P> {
    /// Create a new [`MockTxBuilder`] instance for testing.
    pub fn new(ics26_address: Address, provider: P, signer_address: String) -> Self {
        Self {
            eth_client: EthApiClient::new(provider.clone()),
            ics26_router: routerInstance::new(ics26_address, provider),
            signer_address,
        }
    }
}

#[async_trait::async_trait]
impl<P> TxBuilderService<EthEureka, CosmosSdk> for MockTxBuilder<P>
where
    P: Provider + Clone,
{
    #[tracing::instrument(skip_all)]
    async fn relay_events(
        &self,
        src_events: Vec<EurekaEventWithHeight>,
        dest_events: Vec<EurekaEventWithHeight>,
        src_client_id: String,
        dst_client_id: String,
        src_packet_seqs: Vec<u64>,
        dst_packet_seqs: Vec<u64>,
    ) -> Result<Vec<u8>> {
        let target_block_number = self.eth_client.get_block_number().await?;

        tracing::info!(
            "Relaying events from Ethereum to Cosmos for client {}",
            dst_client_id
        );

        let target_height = Height {
            revision_number: 0,
            revision_height: target_block_number,
        };

        let now_since_unix = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH)?;

        let mut timeout_msgs = cosmos::target_events_to_timeout_msgs(
            dest_events,
            &src_client_id,
            &dst_client_id,
            &dst_packet_seqs,
            &target_height,
            &self.signer_address,
            now_since_unix.as_secs(),
        );

        let (mut recv_msgs, mut ack_msgs) = cosmos::src_events_to_recv_and_ack_msgs(
            src_events,
            &src_client_id,
            &dst_client_id,
            &src_packet_seqs,
            &dst_packet_seqs,
            &target_height,
            &self.signer_address,
            now_since_unix.as_secs(),
        );

        tracing::debug!("Timeout messages: #{}", timeout_msgs.len());
        tracing::debug!("Recv messages: #{}", recv_msgs.len());
        tracing::debug!("Ack messages: #{}", ack_msgs.len());

        cosmos::inject_mock_proofs(&mut recv_msgs, &mut ack_msgs, &mut timeout_msgs);

        let all_msgs = timeout_msgs
            .into_iter()
            .map(|m| Any::from_msg(&m))
            .chain(recv_msgs.into_iter().map(|m| Any::from_msg(&m)))
            .chain(ack_msgs.into_iter().map(|m| Any::from_msg(&m)))
            .collect::<Result<Vec<_>, _>>()?;

        let tx_body = TxBody {
            messages: all_msgs,
            ..Default::default()
        };
        Ok(tx_body.encode_to_vec())
    }
}
