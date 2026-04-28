use std::time::{Duration, Instant};

// Uruchom: cargo test --test rpc_latency_probe -- --nocapture --ignored
// Zmienne środowiskowe:
//   RPC_URL   - endpoint Helius/QuickNode
//   WALLET    - adres portfela Solana

#[tokio::test]
#[ignore]
async fn rpc_latency_probe() {
    let rpc_url = std::env::var("RPC_URL").expect("Ustaw zmienną środowiskową RPC_URL");
    let wallet = std::env::var("WALLET").expect("Ustaw zmienną środowiskową WALLET");

    let client = reqwest::Client::builder()
        .timeout(Duration::from_secs(10))
        .build()
        .expect("Błąd budowania klienta HTTP");

    let request_body = serde_json::json!({
        "jsonrpc": "2.0",
        "id": 1,
        "method": "getSignaturesForAddress",
        "params": [wallet, { "limit": 50 }]
    });

    const PROBES: usize = 50;

    println!("\n=== RPC Latency Probe (równoległy) ===");
    println!("Endpoint : {}", rpc_url);
    println!("Wallet   : {}", wallet);
    println!("Próby    : {} (wysyłane współbieżnie)\n", PROBES);

    // Buduj futures dla wszystkich prób jednocześnie
    let futures: Vec<_> = (0..PROBES)
        .map(|i| {
            let client = client.clone();
            let url = rpc_url.clone();
            let body = request_body.clone();

            async move {
                let start = Instant::now();

                let result = client.post(&url).json(&body).send().await;

                match result {
                    Ok(resp) => {
                        let status = resp.status();
                        match resp.text().await {
                            Ok(text) => {
                                let elapsed_ms = start.elapsed().as_millis();
                                let preview = if text.len() > 80 { &text[..80] } else { &text };
                                println!(
                                    "[{:>2}] {:>6}ms  HTTP {}  {}…",
                                    i + 1,
                                    elapsed_ms,
                                    status,
                                    preview
                                );
                                Some(elapsed_ms)
                            }
                            Err(e) => {
                                println!("[{:>2}] ERROR body: {}", i + 1, e);
                                None
                            }
                        }
                    }
                    Err(e) => {
                        println!("[{:>2}] ERROR send: {}", i + 1, e);
                        None
                    }
                }
            }
        })
        .collect();

    // Uruchom wszystkie równolegle, zmierz całkowity czas
    let wall_start = Instant::now();
    let results = futures::future::join_all(futures).await;
    let wall_ms = wall_start.elapsed().as_millis();

    // Zbierz tylko udane próby
    let mut latencies_ms: Vec<u128> = results.into_iter().flatten().collect();

    if latencies_ms.is_empty() {
        println!("BRAK udanych prób — sprawdź RPC_URL i WALLET");
        return;
    }

    latencies_ms.sort_unstable();
    let n = latencies_ms.len();
    let min = latencies_ms[0];
    let max = latencies_ms[n - 1];
    let p50 = latencies_ms[n / 2];
    let p95 = latencies_ms[(n as f64 * 0.95) as usize - 1];
    let avg = latencies_ms.iter().sum::<u128>() / n as u128;

    println!("\n=== Wyniki ===");
    println!("Udane próby  : {}/{}", n, PROBES);
    println!(
        "Czas ściany  : {}ms  (vs ~{}ms sekwencyjnie)",
        wall_ms,
        avg * PROBES as u128
    );
    println!("MIN          : {}ms", min);
    println!("AVG          : {}ms", avg);
    println!("P50          : {}ms", p50);
    println!("P95          : {}ms", p95);
    println!("MAX          : {}ms", max);
    println!("\nTarget IWIM  : <1ms (120µs)");
    println!("→ RPC MUSI być async prefetch, nie inline w oknie 0-2s\n");
}
