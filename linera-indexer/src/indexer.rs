// Copyright (c) Zefchain Labs, Inc.
// SPDX-License-Identifier: Apache-2.0

//! This module defines the base component of linera-indexer.

use crate::{
    common::{graphiql, IndexerError},
    plugin::Plugin,
    service::{Listener, Service},
};
use async_graphql::{EmptyMutation, EmptySubscription, Object, Schema, SimpleObject};
use async_graphql_axum::{GraphQLRequest, GraphQLResponse};
use async_recursion::async_recursion;
use axum::{extract::Extension, routing::get, Router};
use linera_base::{crypto::CryptoHash, data_types::BlockHeight, identifiers::ChainId};
use linera_chain::data_types::HashedValue;
use linera_views::{
    common::{Context, ContextFromDb, KeyValueStoreClient},
    map_view::MapView,
    register_view::RegisterView,
    set_view::SetView,
    value_splitting::DatabaseConsistencyError,
    views::{RootView, View, ViewError},
};
use std::{collections::BTreeMap, sync::Arc};
use tokio::sync::Mutex;
use tower_http::cors::CorsLayer;
use tracing::info;

#[derive(RootView)]
pub struct StateView<C> {
    chains: MapView<C, ChainId, (CryptoHash, BlockHeight)>,
    plugins: SetView<C, String>,
    initiated: RegisterView<C, bool>,
}

#[derive(Clone)]
pub struct State<C>(Arc<Mutex<StateView<C>>>);

type StateSchema<DB> = Schema<State<ContextFromDb<(), DB>>, EmptyMutation, EmptySubscription>;

pub struct Indexer<DB> {
    pub state: State<ContextFromDb<(), DB>>,
    pub plugins: BTreeMap<String, Box<dyn Plugin<DB>>>,
}

pub enum IndexerCommand {
    Run,
    Schema,
}

impl<DB> Indexer<DB>
where
    DB: KeyValueStoreClient + Clone + Send + Sync + 'static,
    DB::Error: From<bcs::Error>
        + From<DatabaseConsistencyError>
        + Send
        + Sync
        + std::error::Error
        + 'static,
    ViewError: From<DB::Error>,
{
    /// Loads all indexer plugins
    pub async fn load(client: DB) -> Result<Self, IndexerError> {
        let context = ContextFromDb::create(client.clone(), "indexer".as_bytes().to_vec(), ())
            .await
            .map_err(|e| IndexerError::ViewError(e.into()))?;
        let state = State(Arc::new(Mutex::new(StateView::load(context).await?)));
        Ok(Indexer {
            state,
            plugins: BTreeMap::new(),
        })
    }

    /// Processes one hashed value
    pub async fn process_value(
        &self,
        state: &mut StateView<ContextFromDb<(), DB>>,
        value: &HashedValue,
    ) -> Result<(), IndexerError> {
        for plugin in self.plugins.values() {
            plugin.register(value).await?
        }
        let chain_id = value.inner().chain_id();
        let hash = value.hash();
        let height = value.inner().height();
        info!("save {:?}: {:?} ({})", chain_id, hash, height);
        state
            .chains
            .insert(&chain_id, (value.hash(), value.inner().height()))?;
        state.save().await.map_err(IndexerError::ViewError)
    }

    /// Processes one hashed value doing recursion until the last state registered is reached
    #[async_recursion]
    async fn process_rec(
        &self,
        service: &Service,
        state: &mut StateView<ContextFromDb<(), DB>>,
        value: &HashedValue,
        last_hash: Option<CryptoHash>,
    ) -> Result<(), IndexerError> {
        let chain_id = value.inner().chain_id();
        let Some(executed_block) = value.inner().executed_block() else {
            return Ok(());
        };
        match executed_block.block.previous_block_hash {
            None => self.process_value(state, value).await,
            Some(hash) => {
                if Some(hash) != last_hash {
                    let previous_value = service.get_value(chain_id, hash).await?;
                    self.process_rec(service, state, &previous_value, last_hash)
                        .await?
                }
                self.process_value(state, value).await
            }
        }
    }

    /// Main function of the indexer: including a value in the indexer
    pub async fn process(
        &self,
        listener: &Listener,
        value: &HashedValue,
    ) -> Result<(), IndexerError> {
        let chain_id = value.inner().chain_id();
        let hash = value.hash();
        let height = value.inner().height();
        let state = &mut self.state.0.lock().await;
        if height < listener.start {
            return Ok(());
        };
        let last_hash = match state.chains.get(&chain_id).await? {
            None => None,
            Some((last_hash, last_height)) => {
                if last_hash == hash || last_height >= height {
                    return Ok(());
                }
                Some(last_hash)
            }
        };
        info!("process {:?}: {:?} ({})", chain_id, hash, height);
        self.process_rec(&listener.service, state, value, last_hash)
            .await
    }

    /// Produces the GraphQL schema for the indexer or for a plugin
    pub fn sdl(&self, plugin: Option<String>) -> Result<String, IndexerError> {
        match plugin {
            None => Ok(self.state.clone().schema().sdl()),
            Some(plugin) => match self.plugins.get(&plugin) {
                Some(plugin) => Ok(plugin.sdl()),
                None => Err(IndexerError::UnknownPlugin(plugin.to_string())),
            },
        }
    }

    /// Registers a new plugin in the indexer
    pub async fn add_plugin(
        &mut self,
        plugin: impl Plugin<DB> + 'static,
    ) -> Result<(), IndexerError> {
        let name = plugin.name();
        self.plugins
            .insert(name.clone(), Box::new(plugin))
            .map_or_else(|| Ok(()), |_| Err(IndexerError::PluginAlreadyRegistered))?;
        let mut state = self.state.0.lock().await;
        Ok(state.plugins.insert(&name)?)
    }

    /// Handles queries made to the root of the indexer
    async fn handler(schema: Extension<StateSchema<DB>>, req: GraphQLRequest) -> GraphQLResponse {
        schema.execute(req.into_inner()).await.into()
    }

    /// Registers the handler to an axum router
    pub fn route(&self, app: Option<Router>) -> Router {
        let app = app.unwrap_or_else(Router::new);
        app.route("/", get(graphiql).post(Self::handler))
            .layer(Extension(self.state.clone().schema()))
            .layer(CorsLayer::permissive())
    }
}

#[derive(SimpleObject)]
pub struct LastBlock {
    chain: ChainId,
    block: Option<CryptoHash>,
    height: Option<BlockHeight>,
}

#[Object]
impl<C> State<C>
where
    C: Context + Clone + Send + Sync + 'static,
    ViewError: From<C::Error>,
{
    /// Gets the plugins registered in the indexer
    pub async fn plugins(&self) -> Result<Vec<String>, IndexerError> {
        let state = self.0.lock().await;
        Ok(state.plugins.indices().await?)
    }

    /// Gets the last blocks registered for each chains handled by the indexer
    pub async fn state(&self) -> Result<Vec<LastBlock>, IndexerError> {
        let state = self.0.lock().await;
        let chains = state.chains.indices().await?;
        let mut result = Vec::new();
        for chain in chains {
            let block = state.chains.get(&chain).await?;
            result.push(LastBlock {
                chain,
                block: block.map(|b| b.0),
                height: block.map(|b| b.1),
            });
        }
        Ok(result)
    }
}

impl<C> State<C>
where
    C: Context + Clone + Send + Sync + 'static,
    ViewError: From<C::Error>,
{
    pub fn schema(self) -> Schema<Self, EmptyMutation, EmptySubscription> {
        Schema::build(self, EmptyMutation, EmptySubscription).finish()
    }
}
