use crate::{
    common::{Batch, Context, ContextFromDb, KeyValueOperations, SimpleTypeIterator},
    memory::{MemoryContext, MemoryStoreMap},
    views::{HashView, Hasher, View, ViewError},
};
use async_trait::async_trait;
use std::{collections::BTreeMap, fmt::Debug, mem};
use tokio::sync::OwnedMutexGuard;

/// A view that represents the KeyValueOperations
#[derive(Debug, Clone)]
pub struct KeyValueStoreView<C> {
    context: C,
    was_cleared: bool,
    updates: BTreeMap<Vec<u8>, Option<Vec<u8>>>,
}

#[async_trait]
impl<C> View<C> for KeyValueStoreView<C>
where
    C: Context + Send + Sync,
    ViewError: From<C::Error>,
{
    fn context(&self) -> &C {
        &self.context
    }

    async fn load(context: C) -> Result<Self, ViewError> {
        Ok(Self {
            context,
            was_cleared: false,
            updates: BTreeMap::new(),
        })
    }

    fn rollback(&mut self) {
        self.was_cleared = false;
        self.updates.clear();
    }

    fn flush(&mut self, batch: &mut Batch) -> Result<(), ViewError> {
        if self.was_cleared {
            self.was_cleared = false;
            batch.delete_key_prefix(self.context.base_key());
            for (index, update) in mem::take(&mut self.updates) {
                if let Some(value) = update {
                    batch.put_key_value_bytes(self.context.derive_key_bytes(&index), value);
                }
            }
        } else {
            for (index, update) in mem::take(&mut self.updates) {
                match update {
                    None => batch.delete_key(self.context.derive_key_bytes(&index)),
                    Some(value) => {
                        batch.put_key_value_bytes(self.context.derive_key_bytes(&index), value)
                    }
                }
            }
        }
        Ok(())
    }

    fn delete(self, batch: &mut Batch) {
        batch.delete_key_prefix(self.context.base_key());
    }

    fn clear(&mut self) {
        self.was_cleared = true;
        self.updates.clear();
    }
}

impl<C> KeyValueStoreView<C>
where
    C: Send + Context,
    ViewError: From<C::Error>,
{
    pub fn new(context: C) -> Self {
        Self {
            context,
            was_cleared: false,
            updates: BTreeMap::new(),
        }
    }

    pub async fn for_each_index<F>(&self, mut f: F) -> Result<(), ViewError>
    where
        F: FnMut(Vec<u8>) -> Result<(), ViewError> + Send,
    {
        let key_prefix = self.context.base_key();
        let mut iter = self.updates.iter();
        let mut pair = iter.next();
        if !self.was_cleared {
            for index in self
                .context
                .find_stripped_keys_by_prefix(&key_prefix)
                .await?
            {
                loop {
                    match pair {
                        Some((key, value)) => {
                            let key = key.clone();
                            if key < index {
                                if value.is_some() {
                                    f(key)?;
                                }
                                pair = iter.next();
                            } else {
                                if key != index {
                                    f(index)?;
                                } else if value.is_some() {
                                    f(key)?;
                                    pair = iter.next();
                                }
                                break;
                            }
                        }
                        None => {
                            f(index)?;
                            break;
                        }
                    }
                }
            }
        }
        while let Some((key, value)) = pair {
            let key = key.clone();
            if value.is_some() {
                f(key)?;
            }
            pair = iter.next();
        }
        Ok(())
    }

    pub async fn for_each_index_value<F>(&self, mut f: F) -> Result<(), ViewError>
    where
        F: FnMut(Vec<u8>, Vec<u8>) -> Result<(), ViewError> + Send,
    {
        let key_prefix = self.context.base_key();
        let mut iter = self.updates.iter();
        let mut pair = iter.next();
        if !self.was_cleared {
            for (index, index_val) in self
                .context
                .find_stripped_key_values_by_prefix(&key_prefix)
                .await?
            {
                loop {
                    match pair {
                        Some((key, value)) => {
                            let key = key.clone();
                            if key < index {
                                if let Some(value) = value {
                                    f(key, value.to_vec())?;
                                }
                                pair = iter.next();
                            } else {
                                if key != index {
                                    f(index, index_val)?;
                                } else if let Some(value) = value {
                                    f(key, value.to_vec())?;
                                    pair = iter.next();
                                }
                                break;
                            }
                        }
                        None => {
                            f(index, index_val)?;
                            break;
                        }
                    }
                }
            }
        }
        while let Some((key, value)) = pair {
            let key = key.clone();
            if let Some(value) = value {
                f(key, value.to_vec())?;
            }
            pair = iter.next();
        }
        Ok(())
    }

    pub async fn indices(&self) -> Result<Vec<Vec<u8>>, ViewError> {
        let mut indices = Vec::new();
        self.for_each_index(|index: Vec<u8>| {
            indices.push(index);
            Ok(())
        })
        .await?;
        Ok(indices)
    }

    pub async fn get(&self, index: &[u8]) -> Result<Option<Vec<u8>>, ViewError> {
        if let Some(update) = self.updates.get(index) {
            return Ok(update.as_ref().cloned());
        }
        if self.was_cleared {
            return Ok(None);
        }
        let key = self.context.derive_key_bytes(index);
        let value = self.context.read_key_bytes(&key).await?;
        Ok(value)
    }

    /// Set or insert a value.
    pub fn insert(&mut self, index: Vec<u8>, value: Vec<u8>) {
        self.updates.insert(index, Some(value));
    }

    /// Remove a value.
    pub fn remove(&mut self, index: Vec<u8>) {
        if self.was_cleared {
            self.updates.remove(&index);
        } else {
            self.updates.insert(index, None);
        }
    }
}

#[async_trait]
impl<C> KeyValueOperations for KeyValueStoreView<C>
where
    C: Context + Sync + Send,
    ViewError: From<C::Error>,
{
    type Error = ViewError;
    type KeyIterator = SimpleTypeIterator<Vec<u8>, ViewError>;
    type KeyValueIterator = SimpleTypeIterator<(Vec<u8>, Vec<u8>), ViewError>;

    async fn read_key_bytes(&self, key: &[u8]) -> Result<Option<Vec<u8>>, ViewError> {
        if let Some(update) = self.updates.get(key) {
            return Ok(update.clone());
        }
        if self.was_cleared {
            return Ok(None);
        }
        let val = self.context.read_key_bytes(key).await?;
        Ok(val)
    }

    async fn find_stripped_keys_by_prefix(
        &self,
        key_prefix: &[u8],
    ) -> Result<Self::KeyIterator, ViewError> {
        let len = key_prefix.len();
        let key_prefix_full = self.context.derive_key_bytes(key_prefix);
        let mut keys = Vec::new();
        if !self.was_cleared {
            for short_key in self
                .context
                .find_stripped_keys_by_prefix(&key_prefix_full)
                .await?
            {
                let mut key = key_prefix.to_vec();
                key.extend_from_slice(&short_key);
                if !self.updates.contains_key(&key) {
                    keys.push(short_key)
                }
            }
        }
        for (key, value) in &self.updates {
            if value.is_some() && key.starts_with(key_prefix) {
                keys.push(key[len..].to_vec())
            }
        }
        Ok(Self::KeyIterator::new(keys))
    }

    async fn find_stripped_key_values_by_prefix(
        &self,
        key_prefix: &[u8],
    ) -> Result<Self::KeyValueIterator, ViewError> {
        let len = key_prefix.len();
        let key_prefix_full = self.context.derive_key_bytes(key_prefix);
        let mut key_values = Vec::<(Vec<u8>, Vec<u8>)>::new();
        if !self.was_cleared {
            for (short_key, value) in self
                .context
                .find_stripped_key_values_by_prefix(&key_prefix_full)
                .await?
            {
                let mut key = key_prefix.to_vec();
                key.extend_from_slice(&short_key);
                if !self.updates.contains_key(&key) {
                    key_values.push((short_key.to_vec(), value));
                }
            }
        }
        for (key, value) in &self.updates {
            if key.starts_with(key_prefix) {
                if let Some(value) = value {
                    let key_value: (Vec<u8>, Vec<u8>) = (key[len..].to_vec(), value.to_vec());
                    key_values.push(key_value);
                }
            }
        }
        Ok(Self::KeyValueIterator::new(key_values))
    }

    async fn write_batch(&self, batch: Batch) -> Result<(), ViewError> {
        self.context.write_batch(batch).await?;
        Ok(())
    }
}

#[async_trait]
impl<C> HashView<C> for KeyValueStoreView<C>
where
    C: Context + Send + Sync,
    ViewError: From<C::Error>,
{
    type Hasher = sha2::Sha512;

    async fn hash(&mut self) -> Result<<Self::Hasher as Hasher>::Output, ViewError> {
        let mut hasher = Self::Hasher::default();
        let mut count = 0;
        self.for_each_index_value(|index: Vec<u8>, value: Vec<u8>| -> Result<(), ViewError> {
            count += 1;
            hasher.update_with_bytes(&index)?;
            hasher.update_with_bytes(&value)?;
            Ok(())
        })
        .await?;
        hasher.update_with_bcs_bytes(&count)?;
        Ok(hasher.finalize())
    }
}

/// A context that stores all values in memory.
#[cfg(any(test, feature = "test"))]
pub type KeyValueStoreMemoryContext<E> = ContextFromDb<E, KeyValueStoreView<MemoryContext<()>>>;

#[cfg(any(test, feature = "test"))]
impl<E> KeyValueStoreMemoryContext<E> {
    pub fn new(guard: OwnedMutexGuard<MemoryStoreMap>, base_key: Vec<u8>, extra: E) -> Self {
        let context = MemoryContext::new(guard, ());
        let key_value_store_view = KeyValueStoreView::new(context);
        Self {
            db: key_value_store_view,
            base_key,
            extra,
        }
    }
}