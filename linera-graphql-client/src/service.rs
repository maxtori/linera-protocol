// Copyright (c) Zefchain Labs, Inc.
// SPDX-License-Identifier: Apache-2.0

use graphql_client::GraphQLQuery;
use linera_base::{
    crypto::CryptoHash,
    data_types::{Amount, BlockHeight, Timestamp},
    identifiers::{ChainId, Destination, Owner},
};

#[cfg(target_arch = "wasm32")]
mod types {
    use super::{BlockHeight, ChainId, CryptoHash};
    use linera_base::data_types::RoundNumber;
    use serde::{Deserialize, Serialize};
    use serde_json::Value;

    pub type Epoch = Value;
    pub type Message = Value;
    pub type Operation = Value;
    pub type Event = Value;
    pub type Origin = Value;
    pub type UserApplicationDescription = Value;
    pub type ApplicationId = String;

    #[derive(Serialize, Deserialize, Debug, Clone, PartialEq)]
    pub struct Notification {
        pub chain_id: ChainId,
        pub reason: Reason,
    }

    #[derive(Serialize, Deserialize, Debug, Clone, PartialEq)]
    #[allow(clippy::enum_variant_names)]
    pub enum Reason {
        NewBlock {
            height: BlockHeight,
            hash: CryptoHash,
        },
        NewIncomingMessage {
            origin: Origin,
            height: BlockHeight,
        },
        NewRound {
            height: BlockHeight,
            round: RoundNumber,
        },
    }
}

#[cfg(not(target_arch = "wasm32"))]
mod types {
    pub use linera_chain::data_types::{
        notifications::{Notification, Reason},
        Event, Origin,
    };
    pub use linera_execution::{
        committee::Epoch, ApplicationId, Message, Operation, UserApplicationDescription,
    };
}

pub use types::*;

#[derive(GraphQLQuery)]
#[graphql(
    schema_path = "graphql/service_schema.graphql",
    query_path = "graphql/service_requests.graphql",
    response_derives = "Debug, Serialize, Clone"
)]
pub struct Chains;

#[derive(GraphQLQuery)]
#[graphql(
    schema_path = "graphql/service_schema.graphql",
    query_path = "graphql/service_requests.graphql",
    response_derives = "Debug, Serialize, Clone, PartialEq"
)]
pub struct Applications;

#[derive(GraphQLQuery)]
#[graphql(
    schema_path = "graphql/service_schema.graphql",
    query_path = "graphql/service_requests.graphql",
    response_derives = "Debug, Serialize, Clone, PartialEq"
)]
pub struct Blocks;

#[derive(GraphQLQuery)]
#[graphql(
    schema_path = "graphql/service_schema.graphql",
    query_path = "graphql/service_requests.graphql",
    response_derives = "Debug, Serialize, Clone, PartialEq"
)]
pub struct Block;

#[derive(GraphQLQuery)]
#[graphql(
    schema_path = "graphql/service_schema.graphql",
    query_path = "graphql/service_requests.graphql",
    response_derives = "Debug, Serialize, Clone, PartialEq"
)]
pub struct Notifications;

#[derive(GraphQLQuery)]
#[graphql(
    schema_path = "graphql/service_schema.graphql",
    query_path = "graphql/service_requests.graphql",
    response_derives = "Debug, Serialize, Clone"
)]
pub struct Transfer;

#[cfg(not(target_arch = "wasm32"))]
mod from {
    use super::*;
    use linera_chain::data_types::{ExecutedBlock, HashedValue, IncomingMessage, OutgoingMessage};

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
            let incoming_messages = val
                .incoming_messages
                .into_iter()
                .map(IncomingMessage::from)
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
                .map(OutgoingMessage::from)
                .collect();
            ExecutedBlock {
                block: val.block.into(),
                messages,
                state_hash: val.state_hash,
            }
        }
    }

    impl TryFrom<block::BlockBlock> for HashedValue {
        type Error = String;
        fn try_from(val: block::BlockBlock) -> Result<Self, Self::Error> {
            match (val.value.status.as_str(), val.value.executed_block) {
                ("validated", Some(executed_block)) => {
                    Ok(HashedValue::new_validated(executed_block.into()))
                }
                // this constructor needs the "test" feature from linera-service
                ("confirmed", Some(executed_block)) => {
                    Ok(HashedValue::new_confirmed(executed_block.into()))
                }
                _ => Err(val.value.status),
            }
        }
    }
}
