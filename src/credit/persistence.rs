// ----- standard library imports
use std::collections::HashMap;
use std::sync::{Arc, RwLock};
// ----- extra library imports
use anyhow::Result as AnyResult;
use cdk::nuts::nut02 as cdk02;
use uuid::Uuid;
// ----- local modules
// ----- local imports
use crate::credit::{keys, quotes};
use crate::keys::KeysetID;
use crate::TStamp;

use super::quoting_service;

#[derive(Default, Clone)]
pub struct InMemoryQuoteRepository {
    quotes: Arc<RwLock<HashMap<Uuid, quotes::Quote>>>,
}
impl quotes::Repository for InMemoryQuoteRepository {
    fn search_by_bill(&self, bill: &str, endorser: &str) -> AnyResult<Option<quotes::Quote>> {
        Ok(self
            .quotes
            .read()
            .unwrap()
            .iter()
            .find(|quote| quote.1.bill == bill && quote.1.endorser == endorser)
            .map(|(_, q)| q.clone()))
    }

    fn store(&self, quote: quotes::Quote) -> AnyResult<()> {
        self.quotes.write().unwrap().insert(quote.id, quote);
        Ok(())
    }
}

impl quoting_service::QuoteRepository for InMemoryQuoteRepository {
    fn load(&self, id: uuid::Uuid) -> AnyResult<Option<quotes::Quote>> {
        Ok(self.quotes.read().unwrap().get(&id).cloned())
    }

    fn update_if_pending(&self, new: quotes::Quote) -> AnyResult<()> {
        let id = new.id;
        let mut m = self.quotes.write().unwrap();
        let result = m.remove(&id);
        if let Some(old) = result {
            if matches!(old.status, quotes::QuoteStatus::Pending { .. }) {
                m.insert(id, new);
            } else {
                m.insert(id, old);
            }
        }
        Ok(())
    }

    fn list_pendings(&self, since: Option<TStamp>) -> AnyResult<Vec<Uuid>> {
        let a = self
            .quotes
            .read()
            .unwrap()
            .iter()
            .filter(|(_, q)| matches!(q.status, quotes::QuoteStatus::Pending { .. }))
            .filter(|(_, q)| q.submitted >= since.unwrap_or_default())
            .map(|(id, _)| *id)
            .collect();
        Ok(a)
    }
    fn list_accepteds(&self, _since: Option<TStamp>) -> AnyResult<Vec<Uuid>> {
        let a = self
            .quotes
            .read()
            .unwrap()
            .iter()
            .filter(|(_, q)| matches!(q.status, quotes::QuoteStatus::Accepted { .. }))
            .map(|(id, _)| *id)
            .collect();
        Ok(a)
    }
}

#[derive(Default, Clone)]
pub struct InMemoryQuoteKeysRepository {
    keys: Arc<RwLock<HashMap<(KeysetID, Uuid), (cdk::mint::MintKeySetInfo, cdk02::MintKeySet)>>>,
}

impl keys::QuoteKeyRepository for InMemoryQuoteKeysRepository {
    fn store(
        &self,
        qid: Uuid,
        keyset: cdk02::MintKeySet,
        info: cdk::mint::MintKeySetInfo,
    ) -> AnyResult<()> {
        self.keys
            .write()
            .unwrap()
            .insert((KeysetID::from(keyset.id), qid), (info, keyset));
        Ok(())
    }
}

#[derive(Default, Clone)]
pub struct InMemoryMaturityKeysRepository {
    keys: Arc<RwLock<HashMap<KeysetID, (cdk::mint::MintKeySetInfo, cdk02::MintKeySet)>>>,
}

impl keys::MaturityKeyRepository for InMemoryMaturityKeysRepository {
    fn info(&self, kid: &KeysetID) -> AnyResult<Option<cdk::mint::MintKeySetInfo>> {
        let a = self
            .keys
            .read()
            .unwrap()
            .get(kid)
            .map(|(info, _)| info.clone());
        Ok(a)
    }
    fn store(&self, keyset: cdk02::MintKeySet, info: cdk::mint::MintKeySetInfo) -> AnyResult<()> {
        self.keys
            .write()
            .unwrap()
            .insert(KeysetID::from(keyset.id), (info, keyset));
        Ok(())
    }
}
