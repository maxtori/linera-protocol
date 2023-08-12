// Copyright (c) Zefchain Labs, Inc.
// SPDX-License-Identifier: Apache-2.0

//! This module defines the trait for indexer plugins.

use crate::types::IndexerError;
use async_graphql::{EmptyMutation, EmptySubscription, Schema};
use linera_chain::data_types::HashedValue;
use linera_views::common::Context;

#[async_trait::async_trait]
pub trait Plugin {
    type C: Context + Send + Sync + 'static + Clone;

    async fn register(&self, value: &HashedValue) -> Result<(), IndexerError>;

    async fn load(context: Self::C) -> Result<Self, IndexerError>
    where
        Self: Sized;

    fn schema(self) -> Schema<Self, EmptyMutation, EmptySubscription>
    where
        Self: Sized;

    fn name(&self) -> String;
}
