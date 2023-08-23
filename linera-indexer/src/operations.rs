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
use std::sync::Arc;
use tokio::sync::Mutex;
use tracing::info;

#[derive(Deserialize, Serialize, Clone, SimpleObject, InputObject, Debug)]
pub struct OperationKey {
    chain_id: ChainId,
    height: BlockHeight,
    index: usize,
}

impl BcsHashable for OperationKey {}

#[derive(Deserialize, Serialize, Clone, SimpleObject, Debug)]
pub struct ChainOperation {
    key: OperationKey,
    previous_operation: Option<CryptoHash>,
    index: u64,
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
        content: Operation,
        height: BlockHeight,
        index: usize,
        chain_id: ChainId,
    ) -> Result<(), IndexerError> {
        let mut plugin = self.0.lock().await;
        let last_operation = plugin.last.get(&chain_id).await?;
        match last_operation {
            Some(key)
                if ((key.height > height) || (key.height == height) && (key.index > index)) =>
            {
                Ok(())
            }
            last_operation => {
                let previous_operation = last_operation.map(|key| CryptoHash::new(&key));
                let key = OperationKey {
                    chain_id,
                    height,
                    index,
                };
                let hash = CryptoHash::new(&key);
                let index = *plugin.count.get_mut(&chain_id).await?.unwrap_or(&mut 0);
                let operation = ChainOperation {
                    key: key.clone(),
                    previous_operation,
                    index,
                    content,
                };
                info!("register operation for {:?}:\n{:?}", chain_id, operation);
                plugin.operations.insert(&hash, operation.clone())?;
                let count = *plugin.count.get_mut(&chain_id).await?.unwrap_or(&mut 0);
                plugin.count.insert(&chain_id, count + 1)?;
                plugin
                    .last
                    .insert(&chain_id, key)
                    .map_err(IndexerError::ViewError)
            }
        }
    }

    /// Main function of plugin: registers the operations for a hashed value
    pub async fn register(&self, value: &HashedValue) -> Result<(), IndexerError> {
        let Some(executed_block) = value.inner().executed_block() else { return Ok(()) };
        let chain_id = value.inner().chain_id();
        for (i, op) in executed_block.block.operations.iter().enumerate() {
            match self
                .register_operation(op.clone(), executed_block.block.height, i, chain_id)
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

#[derive(OneofObject)]
pub enum OperationKeyKind {
    Hash(CryptoHash),
    Key(OperationKey),
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
