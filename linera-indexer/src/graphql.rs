// Copyright (c) Zefchain Labs, Inc.
// SPDX-License-Identifier: Apache-2.0

use crate::types::IndexerError;
use graphql_client::GraphQLQuery;
use linera_base::{
    crypto::CryptoHash,
    data_types::{BlockHeight, Timestamp},
    identifiers::{ChainId, Destination, Owner},
};
use linera_chain::data_types::{
    notifications::Notification, Event, ExecutedBlock, HashedValue, IncomingMessage, Origin,
    OutgoingMessage,
};
use linera_execution::{committee::Epoch, Message, Operation};

#[derive(GraphQLQuery)]
#[graphql(
    schema_path = "../linera-explorer/graphql/schema.graphql",
    query_path = "../linera-explorer/graphql/notifications.graphql",
    response_derives = "Debug"
)]
pub struct Notifications;

#[derive(GraphQLQuery)]
#[graphql(
    schema_path = "../linera-explorer/graphql/schema.graphql",
    query_path = "../linera-explorer/graphql/block.graphql",
    response_derives = "Debug"
)]
pub struct Block;

#[derive(GraphQLQuery)]
#[graphql(
    schema_path = "../linera-explorer/graphql/schema.graphql",
    query_path = "../linera-explorer/graphql/chains.graphql",
    response_derives = "Debug"
)]
pub struct Chains;

impl From<block::BlockBlockValueExecutedBlockBlockIncomingMessages> for IncomingMessage {
    fn from(val: block::BlockBlockValueExecutedBlockBlockIncomingMessages) -> Self {
        IncomingMessage {
            origin: val.origin,
            event: val.event,
        }
    }
}

impl From<block::BlockBlockValueExecutedBlockBlock> for linera_chain::data_types::Block {
    fn from(val: block::BlockBlockValueExecutedBlockBlock) -> Self {
        let incoming_messages: Vec<IncomingMessage> = val
            .incoming_messages
            .into_iter()
            .map(|b: block::BlockBlockValueExecutedBlockBlockIncomingMessages| b.into())
            .collect();
        linera_chain::data_types::Block {
            chain_id: val.chain_id,
            epoch: val.epoch,
            incoming_messages,
            operations: val.operations,
            height: val.height,
            timestamp: val.timestamp,
            authenticated_signer: val.authenticated_signer,
            previous_block_hash: val.previous_block_hash,
        }
    }
}

impl From<block::BlockBlockValueExecutedBlockMessages> for OutgoingMessage {
    fn from(val: block::BlockBlockValueExecutedBlockMessages) -> Self {
        OutgoingMessage {
            destination: val.destination,
            authenticated_signer: val.authenticated_signer,
            message: val.message,
        }
    }
}

impl From<block::BlockBlockValueExecutedBlock> for ExecutedBlock {
    fn from(val: block::BlockBlockValueExecutedBlock) -> Self {
        let messages: Vec<OutgoingMessage> = val
            .messages
            .into_iter()
            .map(|b: block::BlockBlockValueExecutedBlockMessages| b.into())
            .collect();
        ExecutedBlock {
            block: val.block.into(),
            messages,
            state_hash: val.state_hash,
        }
    }
}

impl TryFrom<block::BlockBlock> for HashedValue {
    type Error = IndexerError;
    fn try_from(val: block::BlockBlock) -> Result<Self, Self::Error> {
        if let Some(executed_block) = val.value.executed_block {
            Ok(HashedValue::new_confirmed(executed_block.into()))
        } else {
            Err(IndexerError::UnexpectedBlockStatus(val.value.status))
        }
    }
}
