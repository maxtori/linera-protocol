// Copyright (c) Zefchain Labs, Inc.
// SPDX-License-Identifier: Apache-2.0

use reqwest::header::InvalidHeaderValue;
use std::net::AddrParseError;
use thiserror::Error;

#[derive(Error, Debug)]
pub enum IndexerError {
    #[error(transparent)]
    ViewError(#[from] linera_views::views::ViewError),
    #[error(transparent)]
    ReqwestError(#[from] reqwest::Error),
    #[error(transparent)]
    GraphQLError(#[from] graphql_ws_client::Error),
    #[error(transparent)]
    TungsteniteError(#[from] async_tungstenite::tungstenite::Error),
    #[error(transparent)]
    InvalidHeader(#[from] InvalidHeaderValue),
    #[error(transparent)]
    ParserError(#[from] AddrParseError),
    #[error(transparent)]
    ServerError(#[from] hyper::Error),
    #[error("Null data: {0:?}")]
    NullData(Option<Vec<graphql_client::Error>>),
    #[error("Block not found: {0}")]
    NotFound(linera_base::crypto::CryptoHash),
    #[error("Unexpected block status: {0}")]
    UnexpectedBlockStatus(String),
}
