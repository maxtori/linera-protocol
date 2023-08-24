// Copyright (c) Zefchain Labs, Inc.
// SPDX-License-Identifier: Apache-2.0

//! This module defines the operations indexer plugin.

use crate::types::IndexerError;
use async_graphql::{
    EmptyMutation, EmptySubscription, InputObject, Object, OneofObject, Schema, SimpleObject,
};
use linera_base::{
    crypto::{BcsHashable, CryptoHash},
    data_types::BlockHeight,
    identifiers::ChainId,
};
use linera_chain::data_types::HashedValue;
use linera_execution::Operation;
use linera_views::{
    common::Context,
    map_view::MapView,
    views::{RootView, View, ViewError},
};
use serde::{Deserialize, Serialize};
use std::{
    cmp::{Ordering, PartialOrd},
    sync::Arc,
};
use tokio::sync::Mutex;
use tracing::info;

#[derive(Deserialize, Serialize, Clone, SimpleObject, Debug, PartialEq)]
pub struct OperationKey {
    chain_id: ChainId,
    height: BlockHeight,
    index: usize,
}

impl PartialOrd for OperationKey {
    fn partial_cmp(&self, other: &Self) -> Option<Ordering> {
        match self.height.cmp(&other.height) {
            Ordering::Equal => Some(self.index.cmp(&other.index)),
            ord => Some(ord),
        }
    }
}

impl BcsHashable for OperationKey {}

#[derive(Deserialize, Serialize, Clone, SimpleObject, Debug)]
pub struct ChainOperation {
    key: OperationKey,
    previous_operation: Option<CryptoHash>,
    index: u64,
    block: CryptoHash,
    content: Operation,
}

#[derive(Deserialize, Serialize, Clone, SimpleObject, Debug)]
pub struct HashedChainOperation {
    operation: ChainOperation,
    hash: CryptoHash,
}

impl HashedChainOperation {
    pub fn new(operation: ChainOperation) -> Self {
        let hash = CryptoHash::new(&operation.key);
        Self { operation, hash }
    }
}

#[derive(RootView)]
pub struct OperationsPluginInternal<C> {
    last: MapView<C, ChainId, OperationKey>,
    count: MapView<C, ChainId, u64>,
    /// ChainOperation MapView indexed by their hash
    operations: MapView<C, CryptoHash, ChainOperation>,
}

#[derive(Clone)]
pub struct OperationsPlugin<C>(Arc<Mutex<OperationsPluginInternal<C>>>);

impl<C> OperationsPlugin<C>
where
    C: Context + Send + Sync + 'static + Clone,
    ViewError: From<C::Error>,
{
    /// Registers an operation and update count and last entries for this chain ID
    async fn register_operation(
        &self,
        key: OperationKey,
        block: CryptoHash,
        content: Operation,
    ) -> Result<(), IndexerError> {
        let mut plugin = self.0.lock().await;
        let last_operation = plugin.last.get(&key.chain_id).await?;
        match last_operation {
            Some(last_key) if last_key >= key => Ok(()),
            last_operation => {
                let previous_operation = last_operation.map(|key| CryptoHash::new(&key));
                let hash = CryptoHash::new(&key);
                let index = *plugin.count.get_mut(&key.chain_id).await?.unwrap_or(&mut 0);
                let operation = ChainOperation {
                    key: key.clone(),
                    previous_operation,
                    index,
                    block,
                    content,
                };
                info!(
                    "register operation for {:?}:\n{:?}",
                    key.chain_id, operation
                );
                plugin.operations.insert(&hash, operation.clone())?;
                let count = *plugin.count.get_mut(&key.chain_id).await?.unwrap_or(&mut 0);
                plugin.count.insert(&key.chain_id, count + 1)?;
                plugin
                    .last
                    .insert(&key.chain_id, key.clone())
                    .map_err(IndexerError::ViewError)
            }
        }
    }

    /// Main function of plugin: registers the operations for a hashed value
    pub async fn register(&self, value: &HashedValue) -> Result<(), IndexerError> {
        let Some(executed_block) = value.inner().executed_block() else { return Ok(()) };
        let chain_id = value.inner().chain_id();
        for (index, content) in executed_block.block.operations.iter().enumerate() {
            let key = OperationKey {
                chain_id,
                height: executed_block.block.height,
                index,
            };
            match self
                .register_operation(key, value.hash(), content.clone())
                .await
            {
                Err(e) => return Err(e),
                Ok(()) => continue,
            }
        }
        let mut plugin = self.0.lock().await;
        plugin.save().await.map_err(IndexerError::ViewError)
    }

    /// Loads the plugin view from context
    pub async fn load(context: C) -> Result<Self, IndexerError> {
        let plugin = OperationsPluginInternal::load(context).await?;
        Ok(OperationsPlugin(Arc::new(Mutex::new(plugin))))
    }

    pub fn schema(self) -> Schema<Self, EmptyMutation, EmptySubscription> {
        Schema::build(self, EmptyMutation, EmptySubscription).finish()
    }
}

#[derive(Deserialize, Serialize, Clone, InputObject, Debug)]
pub struct OperationKeyInput {
    chain_id: ChainId,
    height: BlockHeight,
    index: usize,
}

impl BcsHashable for OperationKeyInput {}

#[derive(OneofObject)]
pub enum OperationKeyKind {
    Hash(CryptoHash),
    Key(OperationKeyInput),
    Last(ChainId),
}

#[Object(name = "OperationsRoot")]
impl<C> OperationsPlugin<C>
where
    C: Context + Send + Sync + 'static + Clone,
    ViewError: From<C::Error>,
{
    /// Gets the operation associated to its hash
    pub async fn operation(
        &self,
        key: OperationKeyKind,
    ) -> Result<Option<HashedChainOperation>, IndexerError> {
        let hash = match key {
            OperationKeyKind::Hash(hash) => hash,
            OperationKeyKind::Last(chain_id) => {
                match self.0.lock().await.last.get(&chain_id).await? {
                    None => return Ok(None),
                    Some(key) => CryptoHash::new(&key),
                }
            }
            OperationKeyKind::Key(key) => CryptoHash::new(&key),
        };
        let operation = self
            .0
            .lock()
            .await
            .operations
            .get(&hash)
            .await
            .map_err(IndexerError::ViewError)?;
        Ok(operation.map(HashedChainOperation::new))
    }

    /// Gets the operations in downward order from an operation hash or from the last block of a chain
    pub async fn operations(
        &self,
        from: OperationKeyKind,
        limit: Option<u32>,
    ) -> Result<Vec<HashedChainOperation>, IndexerError> {
        let mut hash = match from {
            OperationKeyKind::Hash(hash) => Some(hash),
            OperationKeyKind::Last(chain_id) => {
                match self.0.lock().await.last.get(&chain_id).await? {
                    None => return Ok(Vec::new()),
                    Some(key) => Some(CryptoHash::new(&key)),
                }
            }
            OperationKeyKind::Key(key) => Some(CryptoHash::new(&key)),
        };
        let mut result = Vec::new();
        let limit = limit.unwrap_or(20);
        for _ in 0..limit {
            let Some(next_hash) = hash else { break };
            let operation = self.0.lock().await.operations.get(&next_hash).await?;
            match operation {
                None => break,
                Some(op) => {
                    hash = op.previous_operation;
                    result.push(HashedChainOperation::new(op))
                }
            }
        }
        Ok(result)
    }

    /// Gets the number of operations registered for a chain
    pub async fn count(&self, chain_id: ChainId) -> Result<u64, IndexerError> {
        let plugin = self.0.lock().await;
        plugin
            .count
            .get(&chain_id)
            .await
            .map_err(IndexerError::ViewError)
            .map(|opt| opt.unwrap_or(0))
    }

    /// Gets the hash of the last operation registered for a chain
    pub async fn last(&self, chain_id: ChainId) -> Result<Option<CryptoHash>, IndexerError> {
        let plugin = self.0.lock().await;
        plugin
            .last
            .get(&chain_id)
            .await
            .map_err(IndexerError::ViewError)
            .map(|opt| opt.map(|key| CryptoHash::new(&key)))
    }
}
