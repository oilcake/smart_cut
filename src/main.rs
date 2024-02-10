use std::env;

fn main() {
    let args: Vec<String> = env::args().collect();
    let file = &args[1];
    let output = std::process::Command::new("ffprobe")
        .arg("-loglevel")
        .arg("error")
        .arg("-select_streams")
        .arg("v:0")
        .arg("-show_entries")
        .arg("packet=pts_time,flags")
        .arg("-of")
        .arg("csv=print_section=0")
        .arg(file)
        .output()
        .expect("failed");

    println!("================================================================================");
    println!("{}", output.status);
    println!("{:?}", output.stdout);
    let s = String::from_utf8_lossy(&output.stderr);
    for line in s.lines() {
        println!("{}", &line);
    }
}
