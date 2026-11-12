use hakobi::catalog::Catalog;
use serde::Deserialize;

#[derive(Deserialize)]
struct RepoDto {
    full_name: String,
    default_branch: String,
    private: bool,
}

#[tokio::main]
async fn main() {
    let catalog = Catalog::open("/tmp/migdata/catalog.sqlite3").expect("open");
    let app = catalog.load_github_app().expect("load").expect("app present");
    let now = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).unwrap().as_secs();
    let claims = serde_json::json!({"iss": app.app_id.to_string(), "iat": now-60, "exp": now+600});
    let key = jsonwebtoken::EncodingKey::from_rsa_pem(app.private_key_pem.as_bytes()).expect("pem");
    let jwt = jsonwebtoken::encode(&jsonwebtoken::Header::new(jsonwebtoken::Algorithm::RS256), &claims, &key).expect("jwt");
    let http = reqwest::Client::new();
    let tok_res = http.post(format!("https://api.github.com/app/installations/{}/access_tokens", app.installation_id))
        .header("Authorization", format!("Bearer {jwt}"))
        .header("Accept", "application/vnd.github+json")
        .header("User-Agent", "hakobi-probe")
        .send().await.expect("send");
    let tok_json: serde_json::Value = tok_res.json().await.expect("json");
    let token = tok_json["token"].as_str().expect("token").to_owned();
    let mut page = 1;
    loop {
        let res = http.get(format!("https://api.github.com/installation/repositories?per_page=100&page={page}"))
            .header("Authorization", format!("Bearer {token}"))
            .header("Accept", "application/vnd.github+json")
            .header("User-Agent", "hakobi-probe")
            .send().await.expect("send");
        let json: serde_json::Value = res.json().await.expect("json");
        let raw: Vec<serde_json::Value> = serde_json::from_value(json["repositories"].clone()).expect("array");
        for v in &raw {
            if serde_json::from_value::<RepoDto>(v.clone()).is_err() {
                println!("OFFENDER: {} default_branch={} private={}", v["full_name"], v["default_branch"], v["private"]);
            }
        }
        println!("page {page}: {} repos", raw.len());
        if raw.len() < 100 { break; }
        page += 1;
    }
}
