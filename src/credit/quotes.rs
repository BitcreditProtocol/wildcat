// ----- standard library imports
// ----- extra library imports
use anyhow::{Error as AnyError, Result as AnyResult};
use async_trait::async_trait;
use bitcoin::bip32 as btc32;
use bitcoin::hashes::sha256::Hash as Sha256;
use bitcoin::hashes::Hash;
use cdk::nuts::nut00 as cdk00;
use cdk::nuts::nut02 as cdk02;
use rust_decimal::{prelude::ToPrimitive, Decimal};
use thiserror::Error;
use uuid::Uuid;
// ----- local modules
// ----- local imports
use crate::credit::keys::generate_keyset_id_from_bill;
use crate::keys::{sign_with_keys, KeysetID, Result as KeyResult};
use crate::utils;
use crate::TStamp;

// ----- error
pub type Result<T> = std::result::Result<T, Error>;
#[derive(Debug, Error)]
pub enum Error {
    // external errors wrappers
    #[error("keys error {0}")]
    Keys(#[from] crate::keys::Error),
    #[error("credit::keys error {0}")]
    CreditKeys(#[from] crate::credit::keys::Error),
    #[error("quotes repository error {0}")]
    Repository(#[from] AnyError),

    #[error("Quote has been already resolved: {0}")]
    QuoteAlreadyResolved(uuid::Uuid),
    #[error("unknown quote id {0}")]
    UnknownQuoteID(uuid::Uuid),
    #[error("Invalid amount: {0}")]
    InvalidAmount(rust_decimal::Decimal),
}

pub fn generate_path_idx_from_quoteid(quoteid: Uuid) -> btc32::ChildNumber {
    const MAX_INDEX: u32 = 2_u32.pow(31) - 1;
    let sha_qid = Sha256::hash(quoteid.as_bytes());
    let u_qid = u32::from_be_bytes(sha_qid[0..4].try_into().expect("a u32 is 4 bytes"));
    let idx_qid = std::cmp::min(u_qid, MAX_INDEX);
    btc32::ChildNumber::from_hardened_idx(idx_qid).expect("keyset is a valid index")
}

#[derive(Debug, Clone)]
pub enum QuoteStatus {
    Pending {
        blinds: Vec<cdk00::BlindedMessage>,
    },
    Declined,
    Accepted {
        signatures: Vec<cdk00::BlindSignature>,
        ttl: TStamp,
    },
}

#[derive(Debug, Clone)]
pub struct Quote {
    pub status: QuoteStatus,
    pub id: Uuid,
    pub bill: String,
    pub endorser: String,
    pub submitted: TStamp,
}

impl Quote {
    pub fn new(
        bill: String,
        endorser: String,
        blinds: Vec<cdk00::BlindedMessage>,
        submitted: TStamp,
    ) -> Self {
        Self {
            status: QuoteStatus::Pending { blinds },
            id: Uuid::new_v4(),
            bill,
            endorser,
            submitted,
        }
    }

    pub fn decline(&mut self) -> Result<()> {
        if let QuoteStatus::Pending { .. } = self.status {
            self.status = QuoteStatus::Declined;
            Ok(())
        } else {
            Err(Error::QuoteAlreadyResolved(self.id))
        }
    }

    pub fn accept(&mut self, signatures: Vec<cdk00::BlindSignature>, ttl: TStamp) -> Result<()> {
        let QuoteStatus::Pending { .. } = self.status else {
            return Err(Error::QuoteAlreadyResolved(self.id));
        };

        self.status = QuoteStatus::Accepted { signatures, ttl };
        Ok(())
    }
}

// ---------- required traits
#[cfg_attr(test, mockall::automock)]
#[async_trait]
pub trait Repository: Send + Sync {
    async fn load(&self, id: uuid::Uuid) -> AnyResult<Option<Quote>>;
    async fn update_if_pending(&self, quote: Quote) -> AnyResult<()>;
    async fn list_pendings(&self, since: Option<TStamp>) -> AnyResult<Vec<Uuid>>;
    async fn list_accepteds(&self, since: Option<TStamp>) -> AnyResult<Vec<Uuid>>;
    async fn search_by_bill(&self, bill: &str, endorser: &str) -> AnyResult<Option<Quote>>;
    async fn store(&self, quote: Quote) -> AnyResult<()>;
}

#[async_trait]
pub trait KeyFactory: Send + Sync {
    async fn generate(
        &self,
        kid: KeysetID,
        qid: Uuid,
        maturity_date: TStamp,
    ) -> AnyResult<cdk02::MintKeySet>;
}

// ---------- Factory
#[derive(Clone)]
pub struct Factory<Quotes> {
    pub quotes: Quotes,
}

impl<Quotes> Factory<Quotes>
where
    Quotes: Repository,
{
    async fn generate(
        &self,
        bill: String,
        endorser: String,
        blinds: Vec<cdk00::BlindedMessage>,
        submitted: TStamp,
    ) -> AnyResult<uuid::Uuid> {
        let Some(quote) = self.quotes.search_by_bill(&bill, &endorser).await? else {
            let quote = Quote::new(bill, endorser, blinds, submitted);
            let id = quote.id;
            self.quotes.store(quote).await?;
            return Ok(id);
        };

        if let QuoteStatus::Accepted { ttl, .. } = quote.status {
            if ttl < submitted {
                let new = Quote::new(bill, endorser, blinds, submitted);
                let id = new.id;
                self.quotes.store(new).await?;
                return Ok(id);
            }
        }
        Ok(quote.id)
    }
}

// ---------- Service
#[derive(Clone)]
pub struct Service<KeysGen, QuotesRepo> {
    pub keys_gen: KeysGen,
    pub quotes_gen: Factory<QuotesRepo>,
    pub quotes: QuotesRepo,
}

impl<KeysGen, QuotesRepo> Service<KeysGen, QuotesRepo>
where
    QuotesRepo: Repository,
{
    pub async fn lookup(&self, id: uuid::Uuid) -> Result<Quote> {
        self.quotes.load(id).await?.ok_or(Error::UnknownQuoteID(id))
    }

    pub async fn decline(&self, id: uuid::Uuid) -> Result<()> {
        let old = self.quotes.load(id).await?;
        if old.is_none() {
            return Err(Error::UnknownQuoteID(id));
        }
        let mut quote = old.unwrap();
        quote.decline()?;
        self.quotes.update_if_pending(quote).await?;
        Ok(())
    }

    pub async fn list_pendings(&self, since: Option<TStamp>) -> Result<Vec<uuid::Uuid>> {
        self.quotes
            .list_pendings(since)
            .await
            .map_err(Error::Repository)
    }

    pub async fn list_accepteds(&self, since: Option<TStamp>) -> Result<Vec<uuid::Uuid>> {
        self.quotes
            .list_accepteds(since)
            .await
            .map_err(Error::Repository)
    }

    pub async fn enquire(
        &self,
        bill: String,
        endorser: String,
        tstamp: TStamp,
        blinds: Vec<cdk00::BlindedMessage>,
    ) -> Result<uuid::Uuid> {
        self.quotes_gen
            .generate(bill, endorser, blinds, tstamp)
            .await
            .map_err(Error::from)
    }
}

impl<KeysGen, QuotesRepo> Service<KeysGen, QuotesRepo>
where
    KeysGen: KeyFactory,
    QuotesRepo: Repository,
{
    pub async fn accept(
        &self,
        id: uuid::Uuid,
        discount: Decimal,
        now: TStamp,
        ttl: Option<TStamp>,
    ) -> Result<()> {
        let discounted_amount =
            cdk::Amount::from(discount.to_u64().ok_or(Error::InvalidAmount(discount))?);

        let mut quote = self.lookup(id).await?;
        let qid = quote.id;
        let kid = generate_keyset_id_from_bill(&quote.bill, &quote.endorser);
        let QuoteStatus::Pending { ref mut blinds } = quote.status else {
            return Err(Error::QuoteAlreadyResolved(qid));
        };

        let selected_blinds = utils::select_blinds_to_target(discounted_amount, blinds);
        log::warn!("WARNING: we are leaving fees on the table, ... but we don't know how much (eBill data missing)");

        // TODO! maturity date should come from the eBill
        let maturity_date = now + chrono::Duration::days(30);
        let keyset = self.keys_gen.generate(kid, qid, maturity_date).await?;

        let signatures = selected_blinds
            .iter()
            .map(|blind| sign_with_keys(&keyset, blind))
            .collect::<KeyResult<Vec<cdk00::BlindSignature>>>()?;
        let expiration = ttl.unwrap_or(utils::calculate_default_expiration_date_for_quote(now));
        quote.accept(signatures, expiration)?;
        self.quotes.update_if_pending(quote).await?;
        Ok(())
    }
}

#[cfg(test)]
mod tests {

    use super::*;
    use mockall::predicate::*;

    #[tokio::test]
    async fn test_new_quote_request_quote_not_present() {
        let mut repo = MockRepository::new();
        repo.expect_search_by_bill().returning(|_, _| Ok(None));
        repo.expect_store().returning(|_| Ok(()));

        let factory = Factory { quotes: repo };
        let test = factory
            .generate(
                String::from("billID"),
                String::from("endorserID"),
                vec![],
                chrono::Utc::now(),
            )
            .await;
        assert!(test.is_ok());
    }

    #[tokio::test]
    async fn test_new_quote_request_quote_pending() {
        let id = Uuid::new_v4();
        let bill_id = "billID";
        let endorser_id = "endorserID";
        let mut repo = MockRepository::new();
        repo.expect_search_by_bill()
            .with(eq(String::from(bill_id)), eq(String::from(endorser_id)))
            .returning(move |_, _| {
                Ok(Some(Quote {
                    status: QuoteStatus::Pending { blinds: vec![] },
                    id,
                    bill: String::from(bill_id),
                    endorser: String::from(endorser_id),
                    submitted: chrono::Utc::now(),
                }))
            });
        repo.expect_store().returning(|_| Ok(()));

        let factory = Factory { quotes: repo };
        let test_id = factory
            .generate(
                String::from(bill_id),
                String::from(endorser_id),
                vec![],
                chrono::Utc::now(),
            )
            .await;
        assert!(test_id.is_ok());
        assert_eq!(id, test_id.unwrap());
    }

    #[tokio::test]
    async fn test_new_quote_request_quote_declined() {
        let id = Uuid::new_v4();
        let bill_id = "billID";
        let endorser_id = "endorserID";
        let mut repo = MockRepository::new();
        repo.expect_search_by_bill()
            .with(eq(String::from(bill_id)), eq(String::from(endorser_id)))
            .returning(move |_, _| {
                Ok(Some(Quote {
                    status: QuoteStatus::Declined,
                    id,
                    bill: String::from(bill_id),
                    endorser: String::from(endorser_id),
                    submitted: chrono::Utc::now(),
                }))
            });
        repo.expect_store().returning(|_| Ok(()));

        let factory = Factory { quotes: repo };
        let test_id = factory
            .generate(
                String::from(bill_id),
                String::from(endorser_id),
                vec![],
                chrono::Utc::now(),
            )
            .await;
        assert!(test_id.is_ok());
        assert_eq!(id, test_id.unwrap());
    }

    #[tokio::test]
    async fn test_new_quote_request_quote_accepted() {
        let id = Uuid::new_v4();
        let bill_id = "billID";
        let endorser_id = "endorserID";
        let mut repo = MockRepository::new();
        repo.expect_search_by_bill()
            .with(eq(String::from(bill_id)), eq(String::from(endorser_id)))
            .returning(move |_, _| {
                Ok(Some(Quote {
                    status: QuoteStatus::Accepted {
                        signatures: vec![],
                        ttl: chrono::Utc::now() + chrono::Duration::days(1),
                    },
                    id,
                    bill: String::from(bill_id),
                    endorser: String::from(endorser_id),
                    submitted: chrono::Utc::now(),
                }))
            });
        repo.expect_store().returning(|_| Ok(()));

        let factory = Factory { quotes: repo };
        let test_id = factory
            .generate(
                String::from(bill_id),
                String::from(endorser_id),
                vec![],
                chrono::Utc::now(),
            )
            .await;
        assert!(test_id.is_ok());
        assert_eq!(id, test_id.unwrap());
    }

    #[tokio::test]
    async fn test_new_quote_request_quote_accepted_but_expired() {
        let id = Uuid::new_v4();
        let bill_id = "billID";
        let endorser_id = "endorserID";
        let mut repo = MockRepository::new();
        repo.expect_search_by_bill()
            .with(eq(String::from(bill_id)), eq(String::from(endorser_id)))
            .returning(move |_, _| {
                Ok(Some(Quote {
                    status: QuoteStatus::Accepted {
                        signatures: vec![],
                        ttl: chrono::Utc::now(),
                    },
                    id,
                    bill: String::from(bill_id),
                    endorser: String::from(endorser_id),
                    submitted: chrono::Utc::now(),
                }))
            });
        repo.expect_store().returning(|_| Ok(()));

        let factory = Factory { quotes: repo };
        let test_id = factory
            .generate(
                String::from(bill_id),
                String::from(endorser_id),
                vec![],
                chrono::Utc::now() + chrono::Duration::seconds(1),
            )
            .await;
        assert!(test_id.is_ok());
        assert_ne!(id, test_id.unwrap());
    }
}
