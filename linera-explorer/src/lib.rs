// Copyright (c) Zefchain Labs, Inc.
// SPDX-License-Identifier: Apache-2.0

//! This module provides web files to run a block explorer from linera service node.

#![recursion_limit = "256"]

mod entrypoint;
mod graphql;
mod input_type;
mod js_utils;

use anyhow::{anyhow, Result};
use futures::prelude::*;
use graphql::{
    applications::ApplicationsApplications as Application,
    block::BlockBlock as Block,
    blocks::BlocksBlocks as Blocks,
    get_operation::{GetOperationOperation as Operation, OperationKeyKind},
    operations::{OperationKeyKind as OperationsKeyKind, OperationsOperations as Operations},
    Chains,
};
use graphql_client::{reqwest::post_graphql, Response};
use js_utils::{getf, log_str, parse, setf, stringify, SER};
use linera_base::{
    crypto::CryptoHash,
    identifiers::{ChainDescription, ChainId},
};
use once_cell::sync::OnceCell;
use serde::{Deserialize, Serialize};
use serde_json::Value;
use serde_wasm_bindgen::from_value;
use std::str::FromStr;
use url::Url;
use uuid::Uuid;
use wasm_bindgen::prelude::*;
use wasm_bindgen_futures::spawn_local;
use ws_stream_wasm::*;

static WEBSOCKET: OnceCell<WsMeta> = OnceCell::new();

/// Page enum containing info for each page.
#[derive(Serialize, Deserialize, Clone)]
#[serde(rename_all = "lowercase")]
enum Page {
    Unloaded,
    Home {
        blocks: Vec<Blocks>,
        apps: Vec<Application>,
    },
    Blocks(Vec<Blocks>),
    Block(Box<Block>),
    Applications(Vec<Application>),
    Application {
        app: Application,
        queries: Value,
        mutations: Value,
        subscriptions: Value,
    },
    Operations(Vec<Operations>),
    Operation(Operation),
    Error(String),
}

/// Config type dealt with localstorage.
#[wasm_bindgen]
#[derive(Serialize, Deserialize, Clone)]
pub struct Config {
    indexer: String,
    node: String,
    tls: bool,
}

impl Config {
    /// Loads config from local storage.
    fn load() -> Self {
        let default = Config {
            indexer: "localhost:8081".to_string(),
            node: "localhost:8080".to_string(),
            tls: false,
        };
        match web_sys::window()
            .expect("window object not found")
            .local_storage()
        {
            Ok(Some(st)) => match st.get_item("config") {
                Ok(Some(s)) => serde_json::from_str::<Config>(&s).unwrap_or(default),
                _ => default,
            },
            _ => default,
        }
    }
}

/// type for Vue data.
#[derive(Serialize, Deserialize, Clone)]
pub struct Data {
    config: Config,
    page: Page,
    chains: Vec<ChainId>,
    chain: ChainId,
}

/// Initializes Vue data.
#[wasm_bindgen]
pub fn data() -> JsValue {
    let data = Data {
        config: Config::load(),
        page: Page::Unloaded,
        chains: Vec::new(),
        chain: ChainId::from(ChainDescription::Root(0)),
    };
    data.serialize(&SER).unwrap()
}

/// GraphQL query type (for subscriptions).
#[derive(Serialize, Deserialize)]
pub struct GQuery<T> {
    id: Option<String>,
    #[serde(rename = "type")]
    typ: String,
    payload: Option<T>,
}

pub enum Protocol {
    Http,
    Websocket,
}

pub enum AddressKind {
    Node,
    Indexer,
}

fn url(config: &Config, protocol: Protocol, kind: AddressKind) -> String {
    let protocol = match protocol {
        Protocol::Http => "http",
        Protocol::Websocket => "ws",
    };
    let tls = if config.tls { "s" } else { "" };
    let address = match kind {
        AddressKind::Node => &config.node,
        AddressKind::Indexer => &config.indexer,
    };
    format!("{}{}://{}", protocol, tls, address)
}

async fn get_blocks(
    node: &str,
    chain_id: ChainId,
    from: Option<CryptoHash>,
    limit: Option<u32>,
) -> Result<Vec<Blocks>> {
    let client = reqwest::Client::new();
    let variables = graphql::blocks::Variables {
        from,
        chain_id,
        limit: limit.map(|x| x.into()),
    };
    let res = post_graphql::<crate::graphql::Blocks, _>(&client, node, variables).await?;
    match res.data {
        None => Err(anyhow!("null blocks data: {:?}", res.errors)),
        Some(data) => Ok(data.blocks),
    }
}

async fn get_applications(node: &str, chain_id: ChainId) -> Result<Vec<Application>> {
    let client = reqwest::Client::new();
    let variables = graphql::applications::Variables { chain_id };
    let result = post_graphql::<crate::graphql::Applications, _>(&client, node, variables).await?;
    match result.data {
        None => Err(anyhow!("null applications data: {:?}", result.errors)),
        Some(data) => Ok(data.applications),
    }
}

async fn get_operations(indexer: &str, chain_id: ChainId) -> Result<Vec<Operations>> {
    let client = reqwest::Client::new();
    let operations_indexer = format!("{}/operations", indexer);
    let variables = graphql::operations::Variables {
        from: OperationsKeyKind::Last(chain_id),
        limit: None,
    };
    let result =
        post_graphql::<crate::graphql::Operations, _>(&client, &operations_indexer, variables)
            .await?;
    result
        .data
        .ok_or_else(|| anyhow!("null operations data: {:?}", result.errors))
        .map(|data| data.operations)
}

/// Returns the error page.
fn error(error: &anyhow::Error) -> (Page, String) {
    (Page::Error(error.to_string()), "/error".to_string())
}

/// Returns the home page.
async fn home(node: &str, chain_id: ChainId) -> Result<(Page, String)> {
    let blocks = get_blocks(node, chain_id, None, None).await?;
    let apps = get_applications(node, chain_id).await?;
    Ok((Page::Home { blocks, apps }, format!("/?chain={}", chain_id)))
}

/// Returns the blocks page.
async fn blocks(
    node: &str,
    chain_id: ChainId,
    from: Option<CryptoHash>,
    limit: Option<u32>,
) -> Result<(Page, String)> {
    // TODO: limit is not used in the UI, it should be implemented with some path arguments and select input
    let blocks = get_blocks(node, chain_id, from, limit).await?;
    Ok((Page::Blocks(blocks), format!("/blocks?chain={}", chain_id)))
}

/// Returns the block page.
async fn block(node: &str, chain_id: ChainId, hash: Option<CryptoHash>) -> Result<(Page, String)> {
    let client = reqwest::Client::new();
    let variables = graphql::block::Variables { hash, chain_id };
    let result = post_graphql::<crate::graphql::Block, _>(&client, node, variables).await?;
    let data = result.data.ok_or_else(|| anyhow!("no block data found"))?;
    let block = data.block.ok_or_else(|| anyhow!("no block found"))?;
    let hash = block.hash;
    Ok((
        Page::Block(Box::new(block)),
        format!("/block/{}?chain={}", hash, chain_id),
    ))
}

/// Queries wallet chains.
async fn chains(app: &JsValue, node: &str) -> Result<ChainId> {
    let client = reqwest::Client::new();
    let variables = graphql::chains::Variables;
    let result = post_graphql::<Chains, _>(&client, node, variables).await?;
    let chains = result
        .data
        .ok_or_else(|| anyhow!("no data in chains query"))?
        .chains;
    let chains_js = chains
        .list
        .serialize(&SER)
        .expect("failed to serialize ChainIds");
    setf(app, "chains", &chains_js);
    Ok(chains.default.unwrap_or_else(|| match chains.list.get(0) {
        None => ChainId::from(ChainDescription::Root(0)),
        Some(chain_id) => *chain_id,
    }))
}

/// Returns the applications page.
async fn applications(node: &str, chain_id: ChainId) -> Result<(Page, String)> {
    let applications = get_applications(node, chain_id).await?;
    Ok((
        Page::Applications(applications),
        format!("/applications?chain={}", chain_id),
    ))
}

/// Returns the applications page.
async fn operations(indexer: &str, chain_id: ChainId) -> Result<(Page, String)> {
    let operations = get_operations(indexer, chain_id).await?;
    Ok((
        Page::Operations(operations),
        format!("/operations?chain={}", chain_id),
    ))
}

/// Returns the block page.
async fn operation(
    indexer: &str,
    key: Option<CryptoHash>,
    chain_id: ChainId,
) -> Result<(Page, String)> {
    let client = reqwest::Client::new();
    let operations_indexer = format!("{}/operations", indexer);
    let key = match key {
        Some(hash) => OperationKeyKind::Hash(hash),
        None => OperationKeyKind::Last(chain_id),
    };
    let variables = graphql::get_operation::Variables { key };
    let result =
        post_graphql::<crate::graphql::GetOperation, _>(&client, operations_indexer, variables)
            .await?;
    let data = result
        .data
        .ok_or_else(|| anyhow!("no operation data found"))?;
    let operation = data
        .operation
        .ok_or_else(|| anyhow!("no operation found"))?;
    let key = operation.hash;
    Ok((
        Page::Operation(operation),
        format!("/operation/{}?chain={}", key, chain_id),
    ))
}

/// Lists entrypoints for GraphQL queries, mutations or subscriptions.
fn list_entrypoints(types: &[Value], name: &Value) -> Option<Value> {
    types
        .iter()
        .find(|x: &&Value| &x["name"] == name)
        .map(|x| x["fields"].clone())
}

/// Fills recursively GraphQL objects with their type definitions.
fn fill_type(element: &Value, types: &Vec<Value>) -> Value {
    match element {
        Value::Array(array) => Value::Array(
            array
                .iter()
                .map(|elt: &Value| fill_type(elt, types))
                .collect(),
        ),
        Value::Object(object) => {
            let mut object = object.clone();
            let name = element["name"].as_str();
            let kind = element["kind"].as_str();
            let of_type = &element["ofType"];
            match (kind, name, of_type) {
                (Some("OBJECT"), Some(name), _) => {
                    match types.iter().find(|elt: &&Value| elt["name"] == name) {
                        None => (),
                        Some(element_definition) => {
                            let fields: Vec<Value> = element_definition["fields"]
                                .as_array()
                                .unwrap()
                                .iter()
                                .map(|elt| fill_type(elt, types))
                                .collect();
                            object.insert("fields".to_string(), Value::Array(fields));
                        }
                    }
                }
                (Some("INPUT_OBJECT"), Some(name), _) => {
                    match types.iter().find(|elt: &&Value| elt["name"] == name) {
                        None => (),
                        Some(element_definition) => {
                            let fields: Vec<Value> = element_definition["inputFields"]
                                .as_array()
                                .unwrap()
                                .iter()
                                .map(|elt| fill_type(elt, types))
                                .collect();
                            object.insert("inputFields".to_string(), Value::Array(fields));
                        }
                    }
                }
                (Some("ENUM"), Some(name), _) => {
                    match types.iter().find(|elt: &&Value| elt["name"] == name) {
                        None => (),
                        Some(element_definition) => {
                            let values: Vec<Value> = element_definition["enumValues"]
                                .as_array()
                                .unwrap()
                                .iter()
                                .map(|elt| fill_type(elt, types))
                                .collect();
                            object.insert("enumValues".to_string(), Value::Array(values));
                        }
                    }
                }
                (Some("LIST" | "NON_NULL"), Some(name), Value::Null) => {
                    match types.iter().find(|elt: &&Value| elt["name"] == name) {
                        None => (),
                        Some(element_definition) => {
                            object
                                .insert("ofType".to_string(), fill_type(element_definition, types));
                        }
                    }
                }
                _ => (),
            };
            object.insert("ofType".to_string(), fill_type(&element["ofType"], types));
            object.insert("type".to_string(), fill_type(&element["type"], types));
            object.insert("args".to_string(), fill_type(&element["args"], types));
            if let Some("LIST") = kind {
                object.insert("_input".to_string(), Value::Array(Vec::new()));
            }
            if let Some("SCALAR" | "ENUM" | "OBJECT") = kind {
                object.insert("_include".to_string(), Value::Bool(true));
            }
            Value::Object(object)
        }
        elt => elt.clone(),
    }
}

/// Returns the application page.
async fn application(app: Application) -> Result<(Page, String)> {
    let schema = graphql::introspection(&app.link).await?;
    let sch = &schema["data"]["__schema"];
    let types = sch["types"]
        .as_array()
        .expect("introspection types is not an array")
        .clone();
    let queries =
        list_entrypoints(&types, &sch["queryType"]["name"]).unwrap_or(Value::Array(Vec::new()));
    let queries = fill_type(&queries, &types);
    let mutations =
        list_entrypoints(&types, &sch["mutationType"]["name"]).unwrap_or(Value::Array(Vec::new()));
    let mutations = fill_type(&mutations, &types);
    let subscriptions = list_entrypoints(&types, &sch["subscriptionType"]["name"])
        .unwrap_or(Value::Array(Vec::new()));
    let subscriptions = fill_type(&subscriptions, &types);
    let pathname = format!("/application/{}", app.id);
    Ok((
        Page::Application {
            app,
            queries,
            mutations,
            subscriptions,
        },
        pathname,
    ))
}

fn format_bytes(value: &JsValue) -> JsValue {
    let modified_value = value.clone();
    if let Some(object) = js_sys::Object::try_from(value) {
        js_sys::Object::keys(object)
            .iter()
            .for_each(|k: JsValue| match k.as_string() {
                None => (),
                Some(key_str) => {
                    if &key_str == "bytes" {
                        let array: Vec<u8> =
                            js_sys::Uint8Array::from(getf(&modified_value, "bytes")).to_vec();
                        let array_hex = hex::encode(array);
                        let hex_len = array_hex.len();
                        let hex_elided = if hex_len > 128 {
                            // don't show all hex digits if the bytes array is too long
                            format!("{}..{}", &array_hex[0..4], &array_hex[hex_len - 4..])
                        } else {
                            array_hex
                        };
                        setf(&modified_value, "bytes", &JsValue::from_str(&hex_elided))
                    } else {
                        setf(
                            &modified_value,
                            &key_str,
                            &format_bytes(&getf(&modified_value, &key_str)),
                        )
                    }
                }
            });
    };
    modified_value
}

fn page_name_and_args(page: &Page) -> (&str, Vec<(String, String)>) {
    match page {
        Page::Unloaded | Page::Home { .. } => ("", Vec::new()),
        Page::Block(b) => ("block", vec![("block".to_string(), b.hash.to_string())]),
        Page::Blocks { .. } => ("blocks", Vec::new()),
        Page::Applications(_) => ("applications", Vec::new()),
        Page::Application { app, .. } => (
            "application",
            vec![("app".to_string(), stringify(&app.serialize(&SER).unwrap()))],
        ),
        Page::Operations(_) => ("operations", Vec::new()),
        Page::Operation(op) => (
            "operation",
            vec![("operation".to_string(), op.hash.to_string())],
        ),
        Page::Error(_) => ("error", Vec::new()),
    }
}

fn find_arg(args: &[(String, String)], key: &str) -> Option<String> {
    args.iter()
        .find_map(|(k, v)| if k == key { Some(v.clone()) } else { None })
}

fn find_arg_map<T, F, E>(args: &[(String, String)], key: &str, f: F) -> Result<Option<T>>
where
    F: FnOnce(&str) -> Result<T, E>,
    E: std::error::Error + Send + Sync + 'static,
{
    match args
        .iter()
        .find_map(|(k, v)| if k == key { Some(v) } else { None })
    {
        None => Ok(None),
        Some(v) => Ok(Some(f(v)?)),
    }
}

fn chain_id_from_args(
    app: &JsValue,
    data: &Data,
    args: &[(String, String)],
    init: bool,
) -> Result<(ChainId, bool)> {
    match find_arg(args, "chain") {
        None => Ok((data.chain, init)),
        Some(chain_id) => {
            let chain_js: JsValue = chain_id
                .serialize(&SER)
                .expect("failed to serialize ChainId");
            setf(app, "chain", &chain_js);
            Ok(ChainId::from_str(&chain_id).map(|id| (id, id != data.chain || init))?)
        }
    }
}

async fn page(
    page_name: &str,
    node: &str,
    indexer: &str,
    chain_id: ChainId,
    args: &[(String, String)],
) -> Result<(Page, String)> {
    match page_name {
        "" => home(node, chain_id).await,
        "block" => {
            let hash = find_arg_map(args, "block", CryptoHash::from_str)?;
            block(node, chain_id, hash).await
        }
        "blocks" => blocks(node, chain_id, None, Some(20)).await,
        "applications" => applications(node, chain_id).await,
        "application" => match find_arg(args, "app").map(|v| parse(&v)) {
            None => Err(anyhow!("unknown application")),
            Some(app_js) => {
                let app = from_value::<Application>(app_js).unwrap();
                application(app).await
            }
        },
        "operation" => {
            let key = find_arg_map(args, "operation", CryptoHash::from_str)?;
            operation(indexer, key, chain_id).await
        }
        "operations" => operations(indexer, chain_id).await,
        "error" => {
            let msg = find_arg(args, "msg").unwrap_or("unknown error".to_string());
            Err(anyhow::Error::msg(msg))
        }
        _ => Err(anyhow!("unknown page")),
    }
}

/// Main function to switch between Vue pages.
async fn route_aux(
    app: &JsValue,
    data: &Data,
    path: &Option<String>,
    args: &[(String, String)],
    init: bool,
) {
    let chain_info = chain_id_from_args(app, data, args, init);
    let (page_name, args): (&str, Vec<(String, String)>) = match (path, &data.page) {
        (Some(p), _) => (p, args.to_vec()),
        (_, p) => page_name_and_args(p),
    };
    let node = url(&data.config, Protocol::Http, AddressKind::Node);
    let indexer = url(&data.config, Protocol::Http, AddressKind::Indexer);
    let result = match chain_info {
        Err(e) => Err(e),
        Ok((chain_id, chain_changed)) => {
            let page_result = page(page_name, &node, &indexer, chain_id, &args).await;
            if chain_changed {
                if let Some(ws) = WEBSOCKET.get() {
                    let _ = ws.close().await;
                }
                let address = url(&data.config, Protocol::Websocket, AddressKind::Node);
                subscribe_chain(app, &address, chain_id).await;
            };
            page_result
        }
    };
    let (page, new_path) = result.unwrap_or_else(|e| error(&e));
    let page_js = format_bytes(&page.serialize(&SER).unwrap());
    setf(app, "page", &page_js);
    web_sys::window()
        .expect("window object not found")
        .history()
        .expect("history object not found")
        .push_state_with_url(&page_js, &new_path, Some(&new_path))
        .expect("push_state failed");
}

#[wasm_bindgen]
pub async fn route(app: JsValue, path: JsValue, args: JsValue) {
    let path = path.as_string();
    let args = from_value::<Vec<(String, String)>>(args).unwrap_or(Vec::new());
    let msg = format!("route: {} {:?}", path.as_deref().unwrap_or("none"), args);
    log_str(&msg);
    let data = from_value::<Data>(app.clone()).expect("cannot parse Vue data");
    route_aux(&app, &data, &path, &args, false).await
}

#[wasm_bindgen]
pub fn short_crypto_hash(s: String) -> String {
    let hash = CryptoHash::from_str(&s).expect("not a crypto hash");
    format!("{:?}", hash)
}

#[wasm_bindgen]
pub fn short_app_id(s: String) -> String {
    format!("{}..{}..{}..", &s[..4], &s[64..68], &s[152..156])
}

fn set_onpopstate(app: JsValue) {
    let callback = Closure::<dyn FnMut(JsValue)>::new(move |v: JsValue| {
        setf(&app, "page", &getf(&v, "state"));
    });
    web_sys::window()
        .expect("window object not found")
        .set_onpopstate(Some(callback.as_ref().unchecked_ref()));
    callback.forget()
}

/// Subscribes to notifications for one chain
async fn subscribe_chain(app: &JsValue, address: &str, chain: ChainId) {
    let (ws, mut wsio) = WsMeta::connect(
        &format!("{}/ws", address),
        Some(vec!["graphql-transport-ws"]),
    )
    .await
    .expect("cannot connect to websocket");
    wsio.send(WsMessage::Text(
        "{\"type\": \"connection_init\", \"payload\": {}}".to_string(),
    ))
    .await
    .expect("cannot send to websocket");
    wsio.next().await;
    let uuid = Uuid::new_v3(&Uuid::NAMESPACE_DNS, b"linera.dev");
    let payload_query = format!(
        r#"subscription {{ notifications(chainId: \"{}\") }}"#,
        chain
    );
    let query = format!(
        r#"{{ "id": "{}", "type": "subscribe", "payload": {{"query": "{}"}} }}"#,
        uuid, payload_query
    );
    wsio.send(WsMessage::Text(query))
        .await
        .expect("cannot send to websocket");
    let app = app.clone();
    spawn_local(async move {
        while let Some(evt) = wsio.next().await {
            match evt {
                WsMessage::Text(message) => {
                    let graphql_message = serde_json::from_str::<
                        GQuery<Response<graphql::notifications::ResponseData>>,
                    >(&message)
                    .expect("unexpected websocket response");
                    if let Some(payload) = graphql_message.payload {
                        if let Some(message_data) = payload.data {
                            let data =
                                from_value::<Data>(app.clone()).expect("cannot parse vue data");
                            if let graphql::Reason::NewBlock { hash: _hash, .. } =
                                message_data.notifications.reason
                            {
                                if message_data.notifications.chain_id == chain {
                                    route_aux(&app, &data, &None, &Vec::new(), false).await
                                }
                            }
                        }
                        if let Some(errors) = payload.errors {
                            errors.iter().for_each(|e| log_str(&e.to_string()));
                            break;
                        }
                    };
                }
                WsMessage::Binary(_) => (),
            }
        }
    });
    let _ = WEBSOCKET.set(ws);
}

/// Initializes pages and subscribes to notifications.
#[wasm_bindgen]
pub async fn init(app: JsValue, uri: String) {
    console_error_panic_hook::set_once();
    set_onpopstate(app.clone());
    let data = from_value::<Data>(app.clone()).expect("cannot parse vue data");
    let address = url(&data.config, Protocol::Http, AddressKind::Node);
    let default_chain = chains(&app, &address).await;
    match default_chain {
        Err(e) => {
            route_aux(
                &app,
                &data,
                &Some("error".to_string()),
                &[("msg".to_string(), e.to_string())],
                true,
            )
            .await
        }
        Ok(default_chain) => {
            let uri = Url::parse(&uri).expect("failed to parse url");
            let pathname = uri.path();
            let mut args: Vec<(String, String)> = uri.query_pairs().into_owned().collect();
            args.push(("chain".to_string(), default_chain.to_string()));
            let path = match pathname {
                "/blocks" => Some("blocks".to_string()),
                "/applications" => Some("applications".to_string()),
                pathname => match (
                    pathname.strip_prefix("/block/"),
                    pathname.strip_prefix("/application/"),
                ) {
                    (Some(hash), _) => {
                        args.push(("block".to_string(), hash.to_string()));
                        Some("block".to_string())
                    }
                    (_, Some(app_id)) => {
                        let link = format!("{}/applications/{}", address, app_id);
                        let app =
                            serde_json::json!({"id": app_id, "link": link, "description": ""})
                                .to_string();
                        args.push(("app".to_string(), app));
                        Some("application".to_string())
                    }
                    _ => None,
                },
            };
            route_aux(&app, &data, &path, &args, true).await;
        }
    }
}

/// Saves config to local storage.
#[wasm_bindgen]
pub fn save_config(app: JsValue) {
    let data = from_value::<Data>(app).expect("cannot parse vue data");
    if let Ok(Some(storage)) = web_sys::window()
        .expect("window object not found")
        .local_storage()
    {
        storage
            .set_item(
                "config",
                &serde_json::to_string::<Config>(&data.config)
                    .expect("cannot parse localstorage config"),
            )
            .expect("cannot set config");
    }
}
