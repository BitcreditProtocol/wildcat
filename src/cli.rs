// ----- standard library imports
// ----- extra library imports
use crossterm::{cursor, event, execute, terminal};
use futures_util::{stream::StreamExt, TryFutureExt};

// ----- local modules
mod credit;
// ----- local imports
use credit::{admin::ListQuotesReply, admin::LookUpQuoteReply};

const MAIN_MENU: &str = "----- press (r) to refresh ----- (q) to quit";

#[tokio::main]
async fn main() {
    let config = CliConfig::new().expect("config");
    // TODO: login/authenticate as admin
    let accepteds = download_accepted(config.endpoint.clone()).await.unwrap();
    let pendings = download_pendings(config.endpoint.clone()).await.unwrap();
    update_screen(&accepteds, &pendings);

    let mut events = crossterm::event::EventStream::new();
    terminal::enable_raw_mode().unwrap();
    while let Some(event) = events.next().await {
        terminal::disable_raw_mode().unwrap();
        match event {
            Ok(event::Event::Key(event::KeyEvent { code, kind, .. })) => match (code, kind) {
                (event::KeyCode::Char('r'), event::KeyEventKind::Press) => {
                    let accepteds = download_accepted(config.endpoint.clone()).await.unwrap();
                    let pendings = download_pendings(config.endpoint.clone()).await.unwrap();
                    update_screen(&accepteds, &pendings);
                }
                (event::KeyCode::Char('q'), event::KeyEventKind::Press) => {
                    break;
                }
                _ => continue,
            },
            _ => continue,
        }
        terminal::enable_raw_mode().unwrap();
    }
    terminal::disable_raw_mode().unwrap();
}

async fn download_pendings(endpoint: String) -> reqwest::Result<Vec<LookUpQuoteReply>> {
    let client = reqwest::Client::new();
    let ids = client
        .get(endpoint.clone() + "admin/credit/v1/quote/pending")
        .send()
        .and_then(reqwest::Response::json::<ListQuotesReply>)
        .await?;
    download_quotes_from_list(&endpoint, &ids).await
}

async fn download_accepted(endpoint: String) -> reqwest::Result<Vec<LookUpQuoteReply>> {
    let client = reqwest::Client::new();
    let ids = client
        .get(endpoint.clone() + "admin/credit/v1/quote/accepted")
        .send()
        .and_then(reqwest::Response::json::<ListQuotesReply>)
        .await?;
    download_quotes_from_list(&endpoint, &ids).await
}

async fn download_quotes_from_list(
    endpoint: &str,
    ids: &ListQuotesReply,
) -> reqwest::Result<Vec<LookUpQuoteReply>> {
    let client = reqwest::Client::new();
    let mut requests: tokio::task::JoinSet<_> = ids
        .quotes
        .iter()
        .map(|id| {
            let url = endpoint.to_string() + "admin/credit/v1/quote/" + &id.to_string();
            client
                .get(url)
                .send()
                .and_then(reqwest::Response::json::<LookUpQuoteReply>)
        })
        .collect();
    let mut quotes: Vec<LookUpQuoteReply> = Vec::new();
    while let Some(response) = requests.try_join_next() {
        quotes.push(response.expect("have we paniced?!?")?);
    }
    Ok(quotes)
}

fn update_screen(accepted: &[LookUpQuoteReply], pending: &[LookUpQuoteReply]) {
    execute!(std::io::stdout(), terminal::Clear(terminal::ClearType::All)).unwrap();
    execute!(std::io::stdout(), cursor::MoveTo(0, 0)).unwrap();

    println!("wildcat admin dashboard");
    println!("{}", MAIN_MENU);

    println!("\n\n");
    println!("Accepted quotes");
    println!("{0: <25} {1: <25}", "bill ID", "endorser ID");
    for quote in accepted {
        let LookUpQuoteReply::Accepted { bill, endorser, .. } = quote else {
            continue;
        };
        println!("{0: <25} {1: <25}", bill, endorser);
    }
    println!("\n\n");
    println!("Pending quotes");
    println!(
        "{0: <8} {1: <25} {2: <25}",
        "index", "bill ID", "endorser ID"
    );
    for (idx, quote) in pending.iter().enumerate() {
        let LookUpQuoteReply::Pending { bill, endorser, .. } = quote else {
            continue;
        };
        println!("{0: <8} {1: <25} {2: <25}", idx, bill, endorser);
    }
}

#[derive(serde::Deserialize)]
pub struct CliConfig {
    pub endpoint: String,
}
impl CliConfig {
    pub fn new() -> std::result::Result<Self, config::ConfigError> {
        let c = config::Config::builder()
            .add_source(config::File::with_name("cli"))
            .add_source(config::Environment::with_prefix("WILDCAT_ADMIN"))
            .set_default("endpoint", "http://localhost:3338/")?
            .build()?;
        c.try_deserialize()
    }
}
