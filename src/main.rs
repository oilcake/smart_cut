use clap::Parser;
use failure::Error;
mod saw;
mod command;
mod copy;

/// CLI arguments
#[derive(Parser, Debug)]
#[command(name = "smart_cut")]
#[command(author = "Your Name")]
#[command(version = "0.1")]
#[command(about = "Keyframe boundary extractor for video trimming")]
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

fn main() -> Result<(), Error> {
    let args = Args::parse();

    let mut saw = saw::Saw::new(&args.input, args.start, args.end).unwrap();
    saw.seek()?;

    dbg!(&saw);

    command::copy_video_fragment(
        &args.input,
        &args.output,
        saw.first_kf.unwrap(),
        saw.last_kf.unwrap() - saw.first_kf.unwrap()
    )?;

    Ok(())
}
