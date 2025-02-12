use std::collections::BTreeMap;
// ----- standard library imports
use std::collections::HashMap;
// ----- extra library imports
use anyhow::Result as AnyResult;
use async_trait::async_trait;
use cdk::nuts::nut00 as cdk00;
use cdk::nuts::nut01 as cdk01;
use cdk::nuts::nut02 as cdk02;
use surrealdb::Result as SurrealResult;
use surrealdb::{engine::any::Any, Surreal};
use uuid::Uuid;
// ----- local modules
// ----- local imports
use crate::credit::keys as creditkeys;
use crate::keys;
use crate::persistence::surreal::ConnectionConfig;

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
struct DBQuoteKeys {
    qid: surrealdb::Uuid,
    info: cdk::mint::MintKeySetInfo,
    // unpacking MintKeySet because surrealdb doesn't support BTreeMap<K,V> where K is not a String
    unit: cdk00::CurrencyUnit,
    keys: HashMap<String, cdk01::MintKeyPair>,
}

fn keysentry2dbquoteskeys(qid: Uuid, ke: keys::KeysetEntry) -> DBQuoteKeys {
    let (info, keyset) = ke;
    let cdk02::MintKeySet { unit, mut keys, .. } = keyset;
    let mut serialized_keys = HashMap::new();
    while let Some((amount, keypair)) = keys.pop_last() {
        serialized_keys.insert(amount.to_string(), keypair);
    }
    DBQuoteKeys {
        qid,
        info,
        unit,
        keys: serialized_keys,
    }
}

fn dbquoteskeys2keysentry(dbqk: DBQuoteKeys) -> (Uuid, keys::KeysetEntry) {
    let DBQuoteKeys {
        qid,
        info,
        unit,
        keys,
    } = dbqk;
    let mut keysmap: BTreeMap<cdk::Amount, cdk01::MintKeyPair> = BTreeMap::default();
    for (val, keypair) in keys {
        let uval = val.parse::<u64>().expect("Failed to parse amount");
        keysmap.insert(cdk::Amount::from(uval), keypair);
    }
    let keyset = cdk02::MintKeySet {
        id: info.id,
        unit,
        keys: cdk01::MintKeys::new(keysmap),
    };

    (qid, (info, keyset))
}

#[derive(Debug, Clone)]
pub struct QuoteKeysDB {
    pub db: Surreal<surrealdb::engine::any::Any>,
}

impl QuoteKeysDB {
    const DB_TABLE: &'static str = "quotekeys";

    pub async fn new(cfg: ConnectionConfig) -> SurrealResult<Self> {
        let db_connection = Surreal::<Any>::init();
        db_connection.connect(cfg.connection).await?;
        db_connection.use_ns(cfg.namespace).await?;
        db_connection.use_db(cfg.database).await?;
        Ok(Self { db: db_connection })
    }
}

#[async_trait]
impl creditkeys::QuoteBasedRepository for QuoteKeysDB {
    async fn load(&self, _kid: &keys::KeysetID, qid: Uuid) -> AnyResult<Option<keys::KeysetEntry>> {
        let res: Option<DBQuoteKeys> = self.db.select((Self::DB_TABLE, qid)).await?;
        Ok(res.map(|dbqk| dbquoteskeys2keysentry(dbqk).1))
    }

    async fn store(
        &self,
        qid: Uuid,
        keyset: cdk02::MintKeySet,
        info: cdk::mint::MintKeySetInfo,
    ) -> AnyResult<()> {
        let dbqk = keysentry2dbquoteskeys(qid, (info, keyset));
        let _: Option<DBQuoteKeys> = self.db.insert((Self::DB_TABLE, qid)).content(dbqk).await?;
        Ok(())
    }
}
