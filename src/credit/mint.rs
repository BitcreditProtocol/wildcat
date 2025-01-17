// ----- standard library imports
// ----- extra library imports
use rust_decimal::Decimal;
use uuid::Uuid;
// ----- local modules
// ----- local imports
use super::{Error, Result};

type TStamp = chrono::DateTime<chrono::Utc>;

#[derive(Debug, Clone)]
pub enum Quote {
    Pending(Request),
    Declined(Request),
    Accepted(Request, Details),
}

#[derive(Debug, Clone)]
pub struct Request {
    pub id: Uuid,
    pub bill: String,
    pub endorser: String,
    pub tstamp: chrono::DateTime<chrono::Utc>,
}

#[derive(Debug, Clone)]
pub struct Details {
    pub discounted: Decimal,
    pub ttl: TStamp,
}

pub trait QuoteRepository: Send + Sync {
    fn load(&self, id: Uuid) -> Option<Quote>;
    fn list(&self) -> Vec<Uuid>;
    fn store(&self, quote: Quote);
    fn remove(&self, id: Uuid) -> Option<Quote>;
}

#[derive(Clone)]
pub struct Service<Quotes> {
    pub quotes: Quotes,
}

impl<Quotes: QuoteRepository> Service<Quotes> {
    pub fn enquire(&self, bill: String, endorser: String, tstamp: TStamp) -> Result<Uuid> {
        let id = Uuid::new_v4();
        self.quotes.store(Quote::Pending(Request {
            id,
            bill,
            endorser,
            tstamp,
        }));
        Ok(id)
    }

    pub fn lookup(&self, id: Uuid) -> Result<Quote> {
        self.quotes.load(id).ok_or(Error::QuoteNotFound(id))
    }

    pub fn list_pending(&self) -> Result<Vec<Uuid>> {
        Ok(self
            .quotes
            .list()
            .into_iter()
            .flat_map(|id| self.quotes.load(id))
            .filter_map(|quote| {
                if let Quote::Pending(request) = quote {
                    Some(request.id)
                } else {
                    None
                }
            })
            .collect())
    }

    pub fn list_accepted(&self) -> Result<Vec<Uuid>> {
        Ok(self
            .quotes
            .list()
            .into_iter()
            .flat_map(|id| self.quotes.load(id))
            .filter_map(|quote| {
                if let Quote::Accepted(request, _) = quote {
                    Some(request.id)
                } else {
                    None
                }
            })
            .collect())
    }

    /// prune expired and declined quotes from storage
    #[allow(dead_code)]
    pub fn prune_quotes(&mut self, now: TStamp) {
        let ids = self.quotes.list();
        for id in ids {
            let quote = self.quotes.load(id).unwrap();
            match quote {
                Quote::Accepted(_, Details { ttl, .. }) => {
                    if ttl < now {
                        self.quotes.remove(id);
                    }
                }
                Quote::Declined(..) => {
                    self.quotes.remove(id);
                }
                _ => {}
            }
        }
    }

    pub fn decline(&mut self, id: Uuid) -> Result<()> {
        let quote = self.quotes.remove(id).ok_or(Error::QuoteNotFound(id))?;
        if let Quote::Pending(request) = quote {
            self.quotes.store(Quote::Declined(request));
        } else {
            self.quotes.store(quote);
        };
        Ok(())
    }

    pub fn accept(&mut self, id: Uuid, discount: Decimal, ttl: TStamp) -> Result<()> {
        let quote = self.quotes.remove(id).ok_or(Error::QuoteNotFound(id))?;
        let Quote::Pending(request) = quote else {
            self.quotes.store(quote);
            return Err(Error::QuoteAlreadyResolved(id));
        };

        self.quotes.store(Quote::Accepted(
            request,
            Details {
                discounted: discount,
                ttl,
            },
        ));

        Ok(())
    }
}
