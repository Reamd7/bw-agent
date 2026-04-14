fn main() {
    match bw_ui::prompt_master_password() {
        Some(pw) => println!("Got password ({} chars)", pw.len()),
        None => println!("Cancelled"),
    }
}
