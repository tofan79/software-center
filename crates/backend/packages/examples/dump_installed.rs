fn main() {
    let apps = scenter_packages::get_installed().unwrap_or_default();
    println!("TOTAL: {}", apps.len());
    for a in &apps {
        println!(
            "id={} | name={} | pkg={} | source={} | icon={} | ver={}",
            a.id, a.name, a.package_name, a.source, a.icon_path, a.version
        );
    }
}
