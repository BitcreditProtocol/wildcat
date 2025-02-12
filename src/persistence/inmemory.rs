// ----- standard library imports
use std::collections::HashMap;
use std::sync::{Arc, RwLock};
// ----- extra library imports
use anyhow::Result as AnyResult;
use async_trait::async_trait;
use cdk::nuts::nut00 as cdk00;
use cdk::nuts::nut01 as cdk01;
use cdk::nuts::nut02 as cdk02;
use cdk::nuts::nut07 as cdk07;
use uuid::Uuid;
// ----- local modules
// ----- local imports
use crate::credit::{keys as creditkeys, quotes};
use crate::keys;
use crate::keys::{KeysetEntry, KeysetID, Repository};
use crate::swap;
use crate::TStamp;

#[derive(Default, Clone)]
pub struct QuotesIDMap {
    quotes: Arc<RwLock<HashMap<Uuid, quotes::Quote>>>,
}
#[async_trait]
impl quotes::Repository for QuotesIDMap {
    async fn search_by_bill(&self, bill: &str, endorser: &str) -> AnyResult<Option<quotes::Quote>> {
        Ok(self
            .quotes
            .read()
            .unwrap()
            .iter()
            .find(|quote| quote.1.bill == bill && quote.1.endorser == endorser)
            .map(|(_, q)| q.clone()))
    }

    async fn store(&self, quote: quotes::Quote) -> AnyResult<()> {
        self.quotes.write().unwrap().insert(quote.id, quote);
        Ok(())
    }
    async fn load(&self, id: uuid::Uuid) -> AnyResult<Option<quotes::Quote>> {
        Ok(self.quotes.read().unwrap().get(&id).cloned())
    }

    async fn update_if_pending(&self, new: quotes::Quote) -> AnyResult<()> {
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

    async fn list_pendings(&self, since: Option<TStamp>) -> AnyResult<Vec<Uuid>> {
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
    async fn list_accepteds(&self, _since: Option<TStamp>) -> AnyResult<Vec<Uuid>> {
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

type QuoteKeysIndex = (KeysetID, Uuid);

#[derive(Default, Clone)]
pub struct KeysetIDQuoteIDMap {
    keys: Arc<RwLock<HashMap<QuoteKeysIndex, KeysetEntry>>>,
}

#[async_trait]
impl creditkeys::QuoteBasedRepository for KeysetIDQuoteIDMap {
    async fn store(
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

    async fn load(&self, kid: &keys::KeysetID, qid: Uuid) -> AnyResult<Option<keys::KeysetEntry>> {
        let mapkey = (kid.clone(), qid);
        Ok(self.keys.read().unwrap().get(&mapkey).cloned())
    }
}

#[derive(Default, Clone)]
pub struct KeysetIDEntryMap {
    keys: Arc<RwLock<HashMap<KeysetID, KeysetEntry>>>,
}

impl keys::Repository for KeysetIDEntryMap {
    fn info(&self, kid: &KeysetID) -> AnyResult<Option<cdk::mint::MintKeySetInfo>> {
        let a = self
            .keys
            .read()
            .unwrap()
            .get(kid)
            .map(|(info, _)| info.clone());
        Ok(a)
    }
    fn keyset(&self, kid: &KeysetID) -> AnyResult<Option<cdk02::MintKeySet>> {
        let a = self
            .keys
            .read()
            .unwrap()
            .get(kid)
            .map(|(_, keyset)| keyset.clone());
        Ok(a)
    }
    fn load(&self, kid: &KeysetID) -> AnyResult<Option<keys::KeysetEntry>> {
        let a = self.keys.read().unwrap().get(kid).cloned();
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

#[derive(Default, Clone)]
pub struct ProofMap {
    proofs: Arc<RwLock<HashMap<cdk01::PublicKey, cdk07::ProofState>>>,
}

impl swap::ProofRepository for ProofMap {
    fn spend(&self, tokens: &[cdk00::Proof]) -> AnyResult<()> {
        let mut writer = self.proofs.write().unwrap();
        for token in tokens {
            let y = cdk::dhke::hash_to_curve(&token.secret.to_bytes())?;
            let proofstate = cdk07::ProofState {
                y,
                state: cdk07::State::Spent,
                witness: None,
            };
            writer.insert(y, proofstate);
        }
        Ok(())
    }

    fn get_state(&self, tokens: &[cdk00::Proof]) -> AnyResult<Vec<cdk07::State>> {
        let mut states: Vec<cdk07::State> = Vec::new();
        let reader = self.proofs.read().unwrap();
        for token in tokens {
            let y = cdk::dhke::hash_to_curve(&token.secret.to_bytes())?;
            let state = reader.get(&y).map_or(cdk07::State::Unspent, |x| x.state);
            states.push(state);
        }
        Ok(states)
    }
}

#[derive(Default, Clone)]
pub struct KeysetIDEntryMapWithActive {
    keys: KeysetIDEntryMap,
    active: Arc<RwLock<Option<KeysetID>>>,
}

impl keys::Repository for KeysetIDEntryMapWithActive {
    fn info(&self, kid: &KeysetID) -> AnyResult<Option<cdk::mint::MintKeySetInfo>> {
        self.keys.info(kid)
    }

    fn keyset(&self, kid: &KeysetID) -> AnyResult<Option<cdk02::MintKeySet>> {
        self.keys.keyset(kid)
    }

    fn load(&self, kid: &KeysetID) -> AnyResult<Option<KeysetEntry>> {
        self.keys.load(kid)
    }

    fn store(&self, keyset: cdk02::MintKeySet, info: cdk::mint::MintKeySetInfo) -> AnyResult<()> {
        if info.active {
            *self.active.write().unwrap() = Some(KeysetID::from(keyset.id));
        }
        self.keys.store(keyset, info)
    }
}

impl keys::ActiveRepository for KeysetIDEntryMapWithActive {
    fn info_active(&self) -> AnyResult<Option<cdk::mint::MintKeySetInfo>> {
        let kid = *self.active.read().unwrap();
        if let Some(kid) = kid {
            return self.keys.info(&kid);
        }
        Ok(None)
    }

    fn keyset_active(&self) -> AnyResult<Option<cdk02::MintKeySet>> {
        let kid = *self.active.read().unwrap();
        if let Some(kid) = kid {
            return self.keys.keyset(&kid);
        }
        Ok(None)
    }
}
