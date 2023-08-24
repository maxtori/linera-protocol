// Copyright (c) Zefchain Labs, Inc.
// SPDX-License-Identifier: Apache-2.0

use async_graphql::{http::GraphiQLSource, EmptyMutation, EmptySubscription, Schema};
use async_graphql_axum::{GraphQLRequest, GraphQLResponse};
use async_tungstenite::tungstenite::{client::IntoClientRequest, http::HeaderValue};
use axum::{
    extract::Extension,
    http::Uri,
    response::{self, IntoResponse},
    routing::get,
    Router, Server,
};
use futures::StreamExt;
use graphql_client::reqwest::post_graphql;
use graphql_ws_client::{graphql::StreamingOperation, GraphQLClientClientBuilder};
use linera_base::{data_types::BlockHeight, identifiers::ChainId};
use linera_chain::data_types::notifications::Reason;
use linera_indexer::{
    graphql::{chains, notifications, Chains, Notifications},
    operations::OperationsPlugin,
    types::IndexerError,
    Indexer, State,
};
use linera_views::rocks_db::{RocksDbClient, RocksDbContext};
use structopt::StructOpt;
use tokio::select;
use tower_http::cors::CorsLayer;
use tracing::{error, info, warn};

#[derive(StructOpt, Debug, Clone)]
enum IndexerCommand {
    Schema {
        plugin: Option<String>,
    },
    Run {
        /// Chains to index
        chains: Vec<ChainId>,
        /// Indexer plugins (operations, messages, ...)
        #[structopt(long, default_value = "")]
        plugins: String,
    },
}

#[derive(StructOpt, Debug, Clone)]
#[structopt(name = "Linera Indexer", about = "Indexer for Linera Microchain")]
struct IndexerConfig {
    /// The port of the indexer server
    #[structopt(long, default_value = "8081")]
    port: u16,
    /// The port of the service to connect
    #[structopt(long = "service-port", default_value = "8080")]
    service_port: u16,
    /// The address of the service to connect
    #[structopt(long = "service-address", default_value = "localhost")]
    service_address: String,
    /// TLS/SSl for service connection
    #[structopt(long = "tls")]
    tls: bool,
    /// RocksDB storage path
    #[structopt(long, default_value = "./indexer.db")]
    storage: String,
    /// The maximal number of simultaneous stream queries to the database
    #[structopt(long, default_value = "10")]
    max_stream_queries: usize,
    /// Cache size for the RocksDB storage
    #[structopt(long, default_value = "1000")]
    cache_size: usize,
    /// Height to start the indexing
    #[structopt(long, default_value = "0")]
    start: BlockHeight,
    /// Indexer command
    #[structopt(subcommand)]
    command: IndexerCommand,
}

struct TokioSpawner(tokio::runtime::Handle);
impl futures::task::Spawn for TokioSpawner {
    fn spawn_obj(
        &self,
        obj: futures::task::FutureObj<'static, ()>,
    ) -> Result<(), futures::task::SpawnError> {
        self.0.spawn(obj);
        Ok(())
    }
}

enum Protocol {
    Http,
    WebSocket,
}

type Context = RocksDbContext<()>;

fn service_address(config: &IndexerConfig, protocol: Protocol) -> String {
    let protocol = match protocol {
        Protocol::Http => "http",
        Protocol::WebSocket => "ws",
    };
    let tls = if config.tls { "s" } else { "" };
    format!(
        "{}{}://{}:{}",
        protocol, tls, config.service_address, config.service_port
    )
}

/// Connects to the websocket of the service node for a particular chain
async fn connect(
    config: &IndexerConfig,
    indexer: &Indexer<Context>,
    chain_id: ChainId,
) -> Result<ChainId, IndexerError> {
    let mut request =
        format!("{}/ws", service_address(config, Protocol::WebSocket)).into_client_request()?;
    request.headers_mut().insert(
        "Sec-WebSocket-Protocol",
        HeaderValue::from_str("graphql-transport-ws")?,
    );
    let (connection, _) = async_tungstenite::tokio::connect_async(request).await?;
    let (sink, stream) = connection.split();

    let mut client = GraphQLClientClientBuilder::new()
        .build(
            stream,
            sink,
            TokioSpawner(tokio::runtime::Handle::current()),
        )
        .await?;
    let operation: StreamingOperation<Notifications> =
        graphql_ws_client::graphql::StreamingOperation::new(notifications::Variables { chain_id });

    let mut stream = client.streaming_operation(operation).await?;

    while let Some(item) = stream.next().await {
        match item {
            Ok(response) => {
                if let Some(data) = response.data {
                    if let Reason::NewBlock { hash, .. } = data.notifications.reason {
                        if let Ok(block) = indexer.get_value(chain_id, hash).await {
                            indexer.process(&block).await?;
                        }
                    }
                } else {
                    error!("null data from GraphQL WebSocket")
                }
            }
            Err(error) => error!("error in WebSocket stream: {}", error),
        }
    }
    Ok(chain_id)
}

/// Loads indexer from the RocksDb context
async fn load_indexer(
    config: &IndexerConfig,
    plugins: Vec<String>,
    command: linera_indexer::IndexerCommand,
) -> Result<Indexer<Context>, IndexerError> {
    let client = RocksDbClient::new(
        &config.storage,
        config.max_stream_queries,
        config.cache_size,
    );
    let node = service_address(config, Protocol::Http);
    Indexer::load(plugins, client, config.start, node, command).await
}

async fn graphiql(uri: Uri) -> impl IntoResponse {
    response::Html(
        GraphiQLSource::build()
            .endpoint(uri.path())
            .subscription_endpoint("/ws")
            .finish(),
    )
}

type StateSchema = Schema<State<Context>, EmptyMutation, EmptySubscription>;

async fn state_handler(schema: Extension<StateSchema>, req: GraphQLRequest) -> GraphQLResponse {
    schema.execute(req.into_inner()).await.into()
}

type OperationsSchema = Schema<OperationsPlugin<Context>, EmptyMutation, EmptySubscription>;

async fn operations_handler(
    schema: Extension<OperationsSchema>,
    req: GraphQLRequest,
) -> GraphQLResponse {
    schema.execute(req.into_inner()).await.into()
}

/// Creates the GraphQL server for the indexer
async fn server(config: &IndexerConfig, indexer: &Indexer<Context>) -> Result<(), IndexerError> {
    let port = config.port;
    let operations = indexer.operations.clone();
    let app = Router::new();
    let app = match operations {
        None => app
            .route("/", get(graphiql).post(state_handler))
            .layer(Extension(indexer.state.clone().schema()))
            .layer(CorsLayer::permissive()),
        Some(plugin) => app
            .route("/", get(graphiql).post(state_handler))
            .route("/operations", get(graphiql).post(operations_handler))
            .layer(Extension(plugin.schema()))
            .layer(Extension(indexer.state.clone().schema()))
            .layer(CorsLayer::permissive()),
    };
    Server::bind(&format!("127.0.0.1:{}", port).parse()?)
        .serve(app.into_make_service())
        .await?;
    Ok(())
}

#[tokio::main]
async fn main() -> Result<(), IndexerError> {
    let env_filter = tracing_subscriber::EnvFilter::builder()
        .with_default_directive(tracing_subscriber::filter::LevelFilter::INFO.into())
        .from_env_lossy();
    tracing_subscriber::fmt()
        .with_writer(std::io::stderr)
        .with_env_filter(env_filter)
        .init();
    let config = IndexerConfig::from_args();
    match config.clone().command {
        IndexerCommand::Schema { plugin } => {
            let (plugins, plugin) = match plugin {
                None => (Vec::new(), "".to_string()),
                Some(plugin) => (vec![plugin.clone()], plugin.to_string()),
            };
            let indexer =
                load_indexer(&config, plugins, linera_indexer::IndexerCommand::Schema).await?;
            println!("{}", indexer.sdl(&plugin)?);
            Ok(())
        }
        IndexerCommand::Run { plugins, chains } => {
            let plugins: Vec<String> = plugins.split(',').map(|x| x.to_string()).collect();
            info!("config: {:?}", config);
            let indexer =
                load_indexer(&config, plugins, linera_indexer::IndexerCommand::Run).await?;
            let chains = if chains.is_empty() {
                let client = reqwest::Client::new();
                let variables = chains::Variables;
                let node = service_address(&config, Protocol::Http);
                let result = post_graphql::<Chains, _>(&client, node, variables).await?;
                result
                    .data
                    .ok_or(IndexerError::NullData(result.errors))?
                    .chains
                    .list
            } else {
                chains.clone()
            };
            let connections = {
                chains
                    .into_iter()
                    .map(|chain_id| connect(&config, &indexer, chain_id))
            };
            select! {
                result = server(&config, &indexer) => {
                    result.map(|()| warn!("GraphQL server stopped"))
                }
                (result, _, _) = futures::future::select_all(connections.map(Box::pin)) => {
                    result.map(|chain_id| {
                        warn!("Connection to {:?} notifications websocket stopped", chain_id)
                    })
                }
            }
        }
    }
}
