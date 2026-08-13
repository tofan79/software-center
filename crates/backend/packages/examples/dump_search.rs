fn main() {
    for q in ["zed", "mpv", "android", "chrome"] {
        println!("=== query: {}", q);
        match scenter_packages::search(q) {
            Ok(apps) => {
                for a in &apps {
                    println!(
                        "  [{}] {} | pkg={} | installed={} | src={}",
                        a.source,
                        a.name,
                        a.package_name,
                        a.installed,
                        a.summary.chars().take(40).collect::<String>()
                    );
                }
            }
            Err(e) => println!("  ERR: {}", e),
        }
    }
}
