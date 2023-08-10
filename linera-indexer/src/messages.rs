// Copyright (c) Zefchain Labs, Inc.
// SPDX-License-Identifier: Apache-2.0

//! This module defines the messages indexer plugin.

// use crate::{plugin::Plugin, types::IndexerError};
// use async_graphql::{Enum, Object, SimpleObject};
// use async_trait::async_trait;
// use linera_base::{
//     crypto::{BcsHashable, CryptoHash},
//     data_types::BlockHeight,
//     identifiers::ChainId,
// };
// use linera_chain::data_types::HashedValue;
// use linera_execution::Message;
// use linera_views::{
//     common::Context,
//     map_view::MapView,
//     views::{RootView, View, ViewError},
// };
// use serde::{Deserialize, Serialize};
// use std::sync::Arc;
// use tokio::sync::Mutex;
// use tracing::info;

// #[derive(Deserialize, Serialize, Clone, Enum, Debug, Copy, Eq, PartialEq)]
// pub enum MessageKind {
//     In,
//     Out,
// }

// #[derive(Deserialize, Serialize, Clone, SimpleObject, Debug)]
// pub struct BlockMessage {
//     block: CryptoHash,
//     index: usize,
//     content: Message,
//     kind: MessageKind,
// }

// #[derive(Deserialize, Serialize, Clone, SimpleObject, Debug)]
// pub struct ChainMessage {
//     message: BlockMessage,
//     height: BlockHeight,
//     previous_message: Option<(CryptoHash, BlockHeight)>,
//     previous_in_message: Option<(CryptoHash, BlockHeight)>,
//     previous_out_message: Option<(CryptoHash, BlockHeight)>,
// }

// #[derive(RootView)]

// pub struct MessagesPluginInternal<C> {
//     last: MapView<C, ChainId, (CryptoHash, BlockHeight, usize)>,
//     last_in: MapView<C, ChainId, (CryptoHash, BlockHeight, usize)>,
//     last_out: MapView<C, ChainId, (CryptoHash, BlockHeight, usize)>,
//     count_in: MapView<C, ChainId, u32>,
//     count_out: MapView<C, ChainId, u32>,
//     /// OperationView MapView indexed by their hash
//     messages: MapView<C, CryptoHash, ChainMessage>,
// }

// #[derive(Clone)]
// pub struct MessagesPlugin<C>(Arc<Mutex<MessagesPluginInternal<C>>>);

// impl<C> MessagesPlugin<C>
// where
//     C: Context + Send + Sync + 'static + Clone,
//     ViewError: From<C::Error>,
// {
//     /// Registers an operation and update count and last entries for this chain ID
//     async fn register_message(
//         &self,
//         kind: MessageKind,
//         content: Message,
//         block: CryptoHash,
//         height: BlockHeight,
//         index: usize,
//         chain_id: ChainId,
//     ) -> Result<(), IndexerError> {
//         let mut plugin = self.0.lock().await;
//         let last_message = plugin.last.get(&chain_id).await?;
//         match last_message {
//             Some((_last_hash, last_height, last_index))
//                 if ((last_height > height) || (last_height == height) && (last_index > index)) =>
//             {
//                 Ok(())
//             }
//             last_message => {
//                 let previous_operation = last_operation.map(|last| last.0);
//                 let operation = BlockOperation {
//                     block,
//                     index,
//                     content,
//                 };
//                 let key = CryptoHash::new(&operation);
//                 let operation_view = ChainOperation {
//                     operation,
//                     previous_operation,
//                     height,
//                 };
//                 info!(
//                     "register operation for {:?}:\n{:?}",
//                     chain_id, operation_view
//                 );
//                 plugin.operations.insert(&key, operation_view.clone())?;
//                 let count = *plugin.count.get_mut(&chain_id).await?.unwrap_or(&mut 0);
//                 plugin.count.insert(&chain_id, count + 1)?;
//                 plugin.last.insert(&chain_id, (key, height, index))?;
//                 plugin.save().await.map_err(IndexerError::ViewError)
//             }
//         }
//     }
// }
