// ----- standard library imports
// ----- extra library imports
use anyhow::{anyhow, Error as AnyError, Result as AnyResult};
use async_trait::async_trait;
use cdk::nuts::nut00 as cdk00;
use surrealdb::Result as SurrealResult;
use surrealdb::{engine::any::Any, Surreal};
use uuid::Uuid;
// ----- local modules
// ----- local imports
use crate::credit::quotes;
use crate::TStamp;

use super::ConnectionConfig;

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize, strum::Display)]
enum DBQuoteStatus {
    Pending,
    Declined,
    Accepted,
}
impl From<&quotes::QuoteStatus> for DBQuoteStatus {
    fn from(value: &quotes::QuoteStatus) -> Self {
        match value {
            quotes::QuoteStatus::Pending { .. } => Self::Pending,
            quotes::QuoteStatus::Declined => Self::Declined,
            quotes::QuoteStatus::Accepted { .. } => Self::Accepted,
        }
    }
}

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
struct DBQuote {
    quote_id: surrealdb::Uuid, // can't be `id`, reserved world in surreal
    bill: String,
    endorser: String,
    submitted: TStamp,
    status: DBQuoteStatus,
    blinds: Option<Vec<cdk00::BlindedMessage>>,
    signatures: Option<Vec<cdk00::BlindSignature>>,
    ttl: Option<TStamp>,
}

impl From<quotes::Quote> for DBQuote {
    fn from(q: quotes::Quote) -> Self {
        match q.status {
            quotes::QuoteStatus::Pending { blinds } => Self {
                quote_id: q.id,
                bill: q.bill,
                endorser: q.endorser,
                submitted: q.submitted,
                status: DBQuoteStatus::Pending,
                blinds: Some(blinds),
                signatures: None,
                ttl: None,
            },
            quotes::QuoteStatus::Declined => Self {
                quote_id: q.id,
                bill: q.bill,
                endorser: q.endorser,
                submitted: q.submitted,
                status: DBQuoteStatus::Declined,
                blinds: None,
                signatures: None,
                ttl: None,
            },
            quotes::QuoteStatus::Accepted { signatures, ttl } => Self {
                quote_id: q.id,
                bill: q.bill,
                endorser: q.endorser,
                submitted: q.submitted,
                status: DBQuoteStatus::Accepted,
                blinds: None,
                signatures: Some(signatures),
                ttl: Some(ttl),
            },
        }
    }
}

impl TryFrom<DBQuote> for quotes::Quote {
    type Error = AnyError;
    fn try_from(dbq: DBQuote) -> Result<Self, Self::Error> {
        match dbq.status {
            DBQuoteStatus::Pending => Ok(Self {
                id: dbq.quote_id,
                bill: dbq.bill,
                endorser: dbq.endorser,
                submitted: dbq.submitted,
                status: quotes::QuoteStatus::Pending {
                    blinds: dbq.blinds.ok_or_else(|| anyhow!("missing blinds"))?,
                },
            }),
            DBQuoteStatus::Declined => Ok(Self {
                id: dbq.quote_id,
                bill: dbq.bill,
                endorser: dbq.endorser,
                submitted: dbq.submitted,
                status: quotes::QuoteStatus::Declined,
            }),
            DBQuoteStatus::Accepted => Ok(Self {
                id: dbq.quote_id,
                bill: dbq.bill,
                endorser: dbq.endorser,
                submitted: dbq.submitted,
                status: quotes::QuoteStatus::Accepted {
                    signatures: dbq
                        .signatures
                        .ok_or_else(|| anyhow!("missing signatures"))?,
                    ttl: dbq.ttl.ok_or_else(|| anyhow!("missing ttl"))?,
                },
            }),
        }
    }
}

#[derive(Debug, Clone)]
pub struct DB {
    pub db: Surreal<surrealdb::engine::any::Any>,
}

impl DB {
    pub async fn new(cfg: ConnectionConfig) -> SurrealResult<Self> {
        let db_connection = Surreal::<Any>::init();
        db_connection.connect(cfg.connection).await?;
        db_connection.use_ns(cfg.namespace).await?;
        db_connection.use_db(cfg.database).await?;
        Ok(Self { db: db_connection })
    }

    async fn load(&self, qid: Uuid) -> SurrealResult<Option<DBQuote>> {
        self.db.select(("quotes", qid)).await
    }

    async fn store(&self, quote: DBQuote) -> SurrealResult<Option<DBQuote>> {
        self.db
            .insert(("quotes", quote.quote_id))
            .content(quote)
            .await
    }

    async fn list_by_status(
        &self,
        status: DBQuoteStatus,
        since: Option<TStamp>,
    ) -> SurrealResult<Vec<Uuid>> {
        let mut query = self
            .db
            .query("SELECT * FROM quotes WHERE status == $status ORDER BY submitted DESC")
            .bind(("status", status));
        if let Some(since) = since {
            query = query
                .query(" AND submitted >= $since")
                .bind(("since", since));
        }
        query.await?.take("quote_id")
    }

    async fn search_by_bill(&self, bill: &str, endorser: &str) -> SurrealResult<Option<DBQuote>> {
        let results: Vec<DBQuote> = self.db
            .query("SELECT * FROM quotes WHERE bill == $bill AND endorser == $endorser ORDER BY submitted DESC")
            .bind(("bill", bill.to_owned()))
            .bind(("endorser", endorser.to_owned())).await?.take(0)?;
        Ok(results.first().cloned())
    }
}

#[async_trait]
impl quotes::Repository for DB {
    async fn load(&self, qid: uuid::Uuid) -> AnyResult<Option<quotes::Quote>> {
        self.load(qid)
            .await?
            .map(std::convert::TryInto::try_into)
            .transpose()
    }

    async fn update_if_pending(&self, new: quotes::Quote) -> AnyResult<()> {
        if matches!(new.status, quotes::QuoteStatus::Pending { .. }) {
            return Err(anyhow!("cannot update to pending"));
        }
        let recordid = surrealdb::RecordId::from_table_key("quotes", new.id);
        self.db
            .query("UPDATE $rid CONTENT $new WHERE status == $status")
            .bind(("rid", recordid))
            .bind(("new", DBQuote::from(new)))
            .bind(("status", DBQuoteStatus::Pending))
            .await?;
        Ok(())
    }

    async fn list_pendings(&self, since: Option<TStamp>) -> AnyResult<Vec<Uuid>> {
        self.list_by_status(DBQuoteStatus::Pending, since)
            .await
            .map_err(Into::into)
    }

    async fn list_accepteds(&self, since: Option<TStamp>) -> AnyResult<Vec<Uuid>> {
        self.list_by_status(DBQuoteStatus::Accepted, since)
            .await
            .map_err(Into::into)
    }

    async fn search_by_bill(&self, bill: &str, endorser: &str) -> AnyResult<Option<quotes::Quote>> {
        self.search_by_bill(bill, endorser)
            .await?
            .map(std::convert::TryInto::try_into)
            .transpose()
    }

    async fn store(&self, quote: quotes::Quote) -> AnyResult<()> {
        self.store(quote.into()).await?;
        Ok(())
    }
}
