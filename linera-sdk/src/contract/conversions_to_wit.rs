// Copyright (c) Zefchain Labs, Inc.
// SPDX-License-Identifier: Apache-2.0

//! Conversions from types declared in [`linera-sdk`] to types generated by
//! [`wit-bindgen-guest-rust`].

use super::{contract_system_api as wit_system_api, wit_types};
use crate::{ApplicationCallResult, ExecutionResult, SessionCallResult};
use linera_base::{
    crypto::CryptoHash,
    identifiers::{ApplicationId, ChannelName, Destination, EffectId, SessionId},
};
use serde::{de::DeserializeOwned, Serialize};
use std::{fmt::Debug, task::Poll};

impl From<CryptoHash> for wit_system_api::CryptoHash {
    fn from(hash_value: CryptoHash) -> Self {
        let parts = <[u64; 4]>::from(hash_value);

        wit_system_api::CryptoHash {
            part1: parts[0],
            part2: parts[1],
            part3: parts[2],
            part4: parts[3],
        }
    }
}

impl From<CryptoHash> for wit_types::CryptoHash {
    fn from(crypto_hash: CryptoHash) -> Self {
        let parts = <[u64; 4]>::from(crypto_hash);

        wit_types::CryptoHash {
            part1: parts[0],
            part2: parts[1],
            part3: parts[2],
            part4: parts[3],
        }
    }
}

impl From<ApplicationId> for wit_system_api::ApplicationId {
    fn from(application_id: ApplicationId) -> wit_system_api::ApplicationId {
        wit_system_api::ApplicationId {
            bytecode_id: application_id.bytecode_id.effect_id.into(),
            creation: application_id.creation.into(),
        }
    }
}

impl From<SessionId> for wit_system_api::SessionId {
    fn from(session_id: SessionId) -> Self {
        wit_system_api::SessionId {
            application_id: session_id.application_id.into(),
            index: session_id.index,
        }
    }
}

impl From<EffectId> for wit_system_api::EffectId {
    fn from(effect_id: EffectId) -> Self {
        wit_system_api::EffectId {
            chain_id: effect_id.chain_id.0.into(),
            height: effect_id.height.0,
            index: effect_id.index,
        }
    }
}

impl From<log::Level> for wit_system_api::LogLevel {
    fn from(level: log::Level) -> Self {
        match level {
            log::Level::Trace => wit_system_api::LogLevel::Trace,
            log::Level::Debug => wit_system_api::LogLevel::Debug,
            log::Level::Info => wit_system_api::LogLevel::Info,
            log::Level::Warn => wit_system_api::LogLevel::Warn,
            log::Level::Error => wit_system_api::LogLevel::Error,
        }
    }
}

impl<Effect, Value, SessionState> From<ApplicationCallResult<Effect, Value, SessionState>>
    for wit_types::ApplicationCallResult
where
    Effect: Serialize + DeserializeOwned + Debug,
    Value: Serialize,
    SessionState: Serialize,
{
    fn from(result: ApplicationCallResult<Effect, Value, SessionState>) -> Self {
        // TODO(#743): Do we need explicit error handling?
        let value = bcs::to_bytes(&result.value)
            .expect("failed to serialize Value for ApplicationCallResult");

        let create_sessions = result
            .create_sessions
            .into_iter()
            .map(|v| {
                bcs::to_bytes(&v)
                    .expect("failed to serialize session state for ApplicationCallResult")
            })
            .collect();

        wit_types::ApplicationCallResult {
            value,
            execution_result: result.execution_result.into(),
            create_sessions,
        }
    }
}

impl<Effect, Value, SessionState> From<SessionCallResult<Effect, Value, SessionState>>
    for wit_types::SessionCallResult
where
    Effect: Serialize + DeserializeOwned + Debug,
    Value: Serialize,
    SessionState: Serialize,
{
    fn from(result: SessionCallResult<Effect, Value, SessionState>) -> Self {
        let new_state = result.new_state.as_ref().map(|state| {
            // TODO(#743): Do we need explicit error handling?
            bcs::to_bytes(state).expect("session type serialization failed")
        });
        wit_types::SessionCallResult {
            inner: result.inner.into(),
            new_state,
        }
    }
}

impl<Effect> From<ExecutionResult<Effect>> for wit_types::ExecutionResult
where
    Effect: Debug + Serialize + DeserializeOwned,
{
    fn from(result: ExecutionResult<Effect>) -> Self {
        let effects = result
            .effects
            .into_iter()
            .map(|(destination, authenticated, effect)| {
                (
                    destination.into(),
                    authenticated,
                    // TODO(#743): Do we need explicit error handling?
                    bcs::to_bytes(&effect).expect("effect serialization failed"),
                )
            })
            .collect();

        let subscribe = result
            .subscribe
            .into_iter()
            .map(|(subscription, chain_id)| (subscription.into(), chain_id.0.into()))
            .collect();

        let unsubscribe = result
            .unsubscribe
            .into_iter()
            .map(|(subscription, chain_id)| (subscription.into(), chain_id.0.into()))
            .collect();

        wit_types::ExecutionResult {
            effects,
            subscribe,
            unsubscribe,
        }
    }
}

impl From<Destination> for wit_types::Destination {
    fn from(destination: Destination) -> Self {
        match destination {
            Destination::Recipient(chain_id) => {
                wit_types::Destination::Recipient(chain_id.0.into())
            }
            Destination::Subscribers(subscription) => {
                wit_types::Destination::Subscribers(subscription.into())
            }
        }
    }
}

impl From<ChannelName> for wit_types::ChannelName {
    fn from(name: ChannelName) -> Self {
        wit_types::ChannelName {
            name: name.into_bytes(),
        }
    }
}

impl<Effect> From<Poll<Result<ExecutionResult<Effect>, String>>> for wit_types::PollExecutionResult
where
    Effect: DeserializeOwned + Serialize + Debug,
{
    fn from(poll: Poll<Result<ExecutionResult<Effect>, String>>) -> Self {
        use wit_types::PollExecutionResult;
        match poll {
            Poll::Pending => PollExecutionResult::Pending,
            Poll::Ready(Ok(result)) => PollExecutionResult::Ready(Ok(result.into())),
            Poll::Ready(Err(message)) => PollExecutionResult::Ready(Err(message)),
        }
    }
}

impl<Effect, Value, SessionState>
    From<Poll<Result<ApplicationCallResult<Effect, Value, SessionState>, String>>>
    for wit_types::PollCallApplication
where
    Effect: Serialize + DeserializeOwned + Debug,
    Value: Serialize,
    SessionState: Serialize,
{
    fn from(
        poll: Poll<Result<ApplicationCallResult<Effect, Value, SessionState>, String>>,
    ) -> Self {
        use wit_types::PollCallApplication;
        match poll {
            Poll::Pending => PollCallApplication::Pending,
            Poll::Ready(Ok(result)) => PollCallApplication::Ready(Ok(result.into())),
            Poll::Ready(Err(message)) => PollCallApplication::Ready(Err(message)),
        }
    }
}

impl<Effect, Value, SessionState>
    From<Poll<Result<SessionCallResult<Effect, Value, SessionState>, String>>>
    for wit_types::PollCallSession
where
    Effect: Serialize + DeserializeOwned + Debug,
    Value: Serialize,
    SessionState: Serialize,
{
    fn from(poll: Poll<Result<SessionCallResult<Effect, Value, SessionState>, String>>) -> Self {
        use wit_types::PollCallSession;
        match poll {
            Poll::Pending => PollCallSession::Pending,
            Poll::Ready(Ok(result)) => PollCallSession::Ready(Ok(result.into())),
            Poll::Ready(Err(message)) => PollCallSession::Ready(Err(message)),
        }
    }
}
