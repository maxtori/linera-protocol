// Copyright (c) Zefchain Labs, Inc.
// SPDX-License-Identifier: Apache-2.0

//! This module defines the trait for indexer plugins.

use async_trait::async_trait;
use linera_chain::data_types::HashedValue;
use linera_views::common::Context;

use crate::types::IndexerError;

#[async_trait]
pub trait Plugin {
    type C: Context + Send + Sync + 'static + Clone;

    async fn register(&self, value: &HashedValue) -> Result<(), IndexerError>;
    async fn load(context: Self::C) -> Result<Box<Self>, IndexerError>;
}
