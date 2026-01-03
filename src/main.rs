use clap::Parser;
mod saw;

/// CLI arguments
#[derive(Parser, Debug)]
#[command(name = "smart_cut")]
#[command(author = "oilcake")]
#[command(version = "0.1")]
#[command(about = "Almost lossless video cutter", long_about = None)]
pub struct Args {
    /// Input video file path
    #[arg(short, long)]
    pub input: String,

    #[arg(short, long)]
    pub output: String,

    /// Start time in seconds
    #[arg(long)]
    pub start: f64,

    /// End time in seconds
    #[arg(long)]
    pub end: f64,
}

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let args = Args::parse();

    let mut saw = saw::Saw::new(&args.input, &args.output, args.start, args.end).unwrap();
    saw.seek()?;

    dbg!(&saw);

    Ok(())
}
