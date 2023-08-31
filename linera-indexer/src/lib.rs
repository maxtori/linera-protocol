// Copyright (c) Zefchain Labs, Inc.
// SPDX-License-Identifier: Apache-2.0

//! This module defines the linera-indexer library including the main indexer loop,
//! the generic plugin and runner trait and the connection to the node service.

pub mod common;
pub mod graphql;
pub mod indexer;
pub mod operations;
pub mod plugin;
pub mod runner;
pub mod service;

#[cfg(feature = "rocksdb")]
pub mod rocks_db;
