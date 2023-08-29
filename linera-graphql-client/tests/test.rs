// Copyright (c) Zefchain Labs, Inc.
// SPDX-License-Identifier: Apache-2.0

use linera_service::client::{resolve_binary, LocalNetwork, Network, INTEGRATION_TEST_GUARD};
use std::{io::Read, rc::Rc};
use tempfile::tempdir;
use tokio::process::Command;

#[test_log::test(tokio::test)]
async fn test_end_to_end_chains_query() {
    use linera_graphql_client::{
        request,
        service::{chains::Variables, Chains},
    };

    let _guard = INTEGRATION_TEST_GUARD.lock().await;
    let network = Network::Grpc;
    let mut local_net = LocalNetwork::new(network, 4).unwrap();
    let client = local_net.make_client(network);
    local_net.generate_initial_validator_config().await.unwrap();
    client.create_genesis_config().await.unwrap();
    local_net.run().await.unwrap();

    let good_result = {
        let wallet = client.get_wallet();
        (wallet.default_chain(), wallet.chain_ids())
    };
    let mut node_service = client.run_node_service(None).await.unwrap();
    let data = request::<Chains, _>(
        &reqwest::Client::new(),
        &format!("http://localhost:{}/", node_service.port()),
        Variables,
    )
    .await
    .unwrap()
    .chains;
    assert_eq!((data.default, data.list), good_result);
    node_service.assert_is_running();
}

#[test_log::test(tokio::test)]
async fn test_end_to_end_applications_query() {
    use linera_graphql_client::{
        request,
        service::{applications::Variables, Applications},
    };

    let _guard = INTEGRATION_TEST_GUARD.lock().await;
    let network = Network::Grpc;
    let mut local_net = LocalNetwork::new(network, 4).unwrap();
    let client = local_net.make_client(network);
    local_net.generate_initial_validator_config().await.unwrap();
    client.create_genesis_config().await.unwrap();
    local_net.run().await.unwrap();

    let chain_id = client.get_wallet().default_chain().unwrap();
    let mut node_service = client.run_node_service(None).await.unwrap();
    let data = request::<Applications, _>(
        &reqwest::Client::new(),
        &format!("http://localhost:{}/", node_service.port()),
        Variables { chain_id },
    )
    .await
    .unwrap()
    .applications;
    assert_eq!(data, Vec::new());
    node_service.assert_is_running();
}

#[test_log::test(tokio::test)]
async fn test_end_to_end_blocks_query() {
    use linera_graphql_client::{
        request,
        service::{blocks::Variables, Blocks},
    };

    let _guard = INTEGRATION_TEST_GUARD.lock().await;
    let network = Network::Grpc;
    let mut local_net = LocalNetwork::new(network, 4).unwrap();
    let client = local_net.make_client(network);
    local_net.generate_initial_validator_config().await.unwrap();
    client.create_genesis_config().await.unwrap();
    local_net.run().await.unwrap();

    let chain_id = client.get_wallet().default_chain().unwrap();
    let mut node_service = client.run_node_service(None).await.unwrap();
    let data = request::<Blocks, _>(
        &reqwest::Client::new(),
        &format!("http://localhost:{}/", node_service.port()),
        Variables {
            chain_id,
            from: None,
            limit: None,
        },
    )
    .await
    .unwrap()
    .blocks;
    assert_eq!(data, Vec::new());
    node_service.assert_is_running();
}

#[test_log::test(tokio::test)]
async fn test_end_to_end_block_query() {
    use linera_graphql_client::{
        request,
        service::{block::Variables, Block},
    };

    let _guard = INTEGRATION_TEST_GUARD.lock().await;
    let network = Network::Grpc;
    let mut local_net = LocalNetwork::new(network, 4).unwrap();
    let client = local_net.make_client(network);
    local_net.generate_initial_validator_config().await.unwrap();
    client.create_genesis_config().await.unwrap();
    local_net.run().await.unwrap();

    let chain_id = client.get_wallet().default_chain().unwrap();
    let mut node_service = client.run_node_service(None).await.unwrap();
    let data = request::<Block, _>(
        &reqwest::Client::new(),
        &format!("http://localhost:{}/", node_service.port()),
        Variables {
            chain_id,
            hash: None,
        },
    )
    .await
    .unwrap()
    .block;
    assert_eq!(data, None);
    node_service.assert_is_running();
}

#[test_log::test(tokio::test)]
async fn test_check_service_schema() {
    let tmp_dir = Rc::new(tempdir().unwrap());
    let path = resolve_binary("linera-schema-export", Some("linera-service"))
        .await
        .unwrap();
    let mut command = Command::new(path);
    let output = command
        .current_dir(tmp_dir.path().canonicalize().unwrap())
        .output()
        .await
        .unwrap();
    let service_schema = String::from_utf8(output.stdout).unwrap();
    let mut file_base = std::fs::File::open("graphql/service_schema.graphql").unwrap();
    let mut graphql_schema = String::new();
    file_base.read_to_string(&mut graphql_schema).unwrap();
    assert_eq!(graphql_schema, service_schema, "\nGraphQL service schema has changed -> regenerate schema following steps in linera-graphql-client/README.md\n")
}

#[test_log::test(tokio::test)]
async fn test_check_indexer_schema() {
    let tmp_dir = Rc::new(tempdir().unwrap());
    let path = resolve_binary("linera-indexer", Some("linera-indexer"))
        .await
        .unwrap();
    let mut command = Command::new(path);
    let output = command
        .current_dir(tmp_dir.path().canonicalize().unwrap())
        .arg("schema")
        .output()
        .await
        .unwrap();
    let indexer_schema = String::from_utf8(output.stdout).unwrap();
    let mut file_base = std::fs::File::open("graphql/indexer_schema.graphql").unwrap();
    let mut graphql_schema = String::new();
    file_base.read_to_string(&mut graphql_schema).unwrap();
    assert_eq!(graphql_schema, indexer_schema, "\nGraphQL indexer schema has changed -> regenerate schema following steps in linera-graphql-client/README.md\n")
}

#[test_log::test(tokio::test)]
async fn test_check_indexer_operations_schema() {
    let tmp_dir = Rc::new(tempdir().unwrap());
    let path = resolve_binary("linera-indexer", Some("linera-indexer"))
        .await
        .unwrap();
    let mut command = Command::new(path);
    let output = command
        .current_dir(tmp_dir.path().canonicalize().unwrap())
        .args(["schema", "operations"])
        .output()
        .await
        .unwrap();
    let service_schema = String::from_utf8(output.stdout).unwrap();
    let mut file_base = std::fs::File::open("graphql/operations_schema.graphql").unwrap();
    let mut graphql_schema = String::new();
    file_base.read_to_string(&mut graphql_schema).unwrap();
    assert_eq!(graphql_schema, service_schema, "\nGraphQL indexer operations schema has changed -> regenerate schema following steps in linera-graphql-client/README.md\n")
}
