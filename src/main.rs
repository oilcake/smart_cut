use clap::Parser;
mod reencoder;
mod saw;

use reencoder::Transcoder;
use std::{collections::HashMap, isize};

use ffmpeg::{codec, encoder, format, log, media, Rational};
use ffmpeg_next::{self as ffmpeg, media::Type, Error};

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
    //
    //     let mut saw = saw::Saw::new(&args.input, &args.output, args.start, args.end).unwrap();
    //     saw.seek()?;
    //
    //     dbg!(&saw);
    //
    //     saw.saw()?;
    //
    //     Ok(())
    // }
    let input_file = &args.input;
    let output_file = &args.output;

    ffmpeg::init().unwrap();
    log::set_level(log::Level::Info);

    let ictx = format::input(&input_file).unwrap();
    let octx = format::output(&output_file).unwrap();

    format::context::input::dump(&ictx, 0, Some(&input_file));

    let mut saw2 = Saw2::new(ictx, octx, args.start, args.end);
    saw2.seek()?;
    dbg!(&saw2);
    saw2.setup_transcoders();
    saw2.write_header();

    saw2.reencode_between_timestamps(args.start, saw2.first_kf.unwrap(), false)?;
    // saw2.reencode_between_timestamps(saw2.first_kf.unwrap(), saw2.last_kf.unwrap(), true)?;
    // saw2.copy_packets_between_keyframes()?;

    saw2.finalize();
    Ok(())
}

struct Saw2 {
    ictx: ffmpeg::format::context::Input,
    octx: ffmpeg::format::context::Output,
    in_str_time_bases: Vec<Rational>,
    out_str_time_bases: Vec<Rational>,
    stream_mapping: Vec<isize>,
    current_pts: Vec<i64>,
    current_dts: Vec<i64>,
    transcoders: HashMap<usize, Transcoder>,

    pub start: f64,
    pub first_kf: Option<f64>,
    pub last_kf: Option<f64>,
    end: f64,
}

impl Saw2 {
    fn new(
        ictx: ffmpeg::format::context::Input,
        mut octx: ffmpeg::format::context::Output,
        start: f64,
        end: f64,
    ) -> Self {
        let mut stream_mapping: Vec<isize> = vec![0; ictx.nb_streams() as _];
        let mut in_str_time_bases = vec![Rational(0, 0); ictx.nb_streams() as _];
        let out_str_time_bases = vec![Rational(0, 0); ictx.nb_streams() as _];
        let current_pts: Vec<i64> = vec![0; ictx.nb_streams() as _];
        let current_dts: Vec<i64> = vec![0; ictx.nb_streams() as _];
        let transcoders = HashMap::new();
        let mut out_str_index = 0;
        for ist in ictx.streams() {
            let in_str_index = ist.index();
            let in_str_medium = ist.parameters().medium();
            // rule out unsupported streams
            if in_str_medium != media::Type::Audio
                && in_str_medium != media::Type::Video
                && in_str_medium != media::Type::Subtitle
            {
                stream_mapping[in_str_index] = -1;
                continue;
            }
            // if stream type is supported
            stream_mapping[in_str_index] = out_str_index;
            in_str_time_bases[in_str_index] = ist.time_base();
            if in_str_medium == media::Type::Video {
                let decoder = ffmpeg::codec::context::Context::from_parameters(ist.parameters())
                    .unwrap()
                    .decoder()
                    .video()
                    .unwrap();
                let codec = encoder::find(decoder.codec().unwrap().id());
                let mut ost = octx.add_stream(codec).unwrap();
                let encoder = codec::context::Context::new_with_codec(
                    codec.ok_or(ffmpeg::Error::InvalidData).unwrap(),
                )
                .encoder()
                .video()
                .unwrap();
                ost.set_parameters(&encoder);
            } else {
                // Set up for stream copy for non-video stream.
                let mut ost = octx.add_stream(encoder::find(codec::Id::None)).unwrap();
                ost.set_parameters(ist.parameters());
                // We need to set codec_tag to 0 lest we run into incompatible codec tag
                // issues when muxing into a different container format. Unfortunately
                // there's no high level API to do this (yet).
                unsafe {
                    (*ost.parameters().as_mut_ptr()).codec_tag = 0;
                }
            }
            out_str_index += 1;
        }

        Saw2 {
            ictx,
            octx,
            in_str_time_bases,
            out_str_time_bases,
            stream_mapping,
            transcoders: transcoders,
            current_pts,
            current_dts,
            start,
            first_kf: None,
            last_kf: None,
            end,
        }
    }
    fn setup_transcoders(&mut self) {
        if self.transcoders.len() > 0 {
            panic!("You have to remove transcoders first");
        }
        let best_video_stream_index = self
            .ictx
            .streams()
            .best(media::Type::Video)
            .map(|stream| stream.index());
        for ist in self.ictx.streams() {
            let in_str_index = ist.index();
            let in_str_medium = ist.parameters().medium();
            // rule out unsupported streams
            if in_str_medium != media::Type::Audio
                && in_str_medium != media::Type::Video
                && in_str_medium != media::Type::Subtitle
            {
                continue;
            }
            let out_str_index = self.stream_mapping[in_str_index];
            let global_header = self
                .octx
                .format()
                .flags()
                .contains(format::Flags::GLOBAL_HEADER);
            if in_str_medium == media::Type::Video {
                let transcoder = Transcoder::new(
                    &ist,
                    out_str_index as _,
                    Some(in_str_index) == best_video_stream_index,
                    global_header,
                )
                .unwrap();
                let mut ost = self.octx.stream_mut(out_str_index as _).unwrap();
                ost.set_parameters(transcoder.encoder());
                // Initialize transcoder for video stream.
                self.transcoders.insert(in_str_index, transcoder);
            }
        }
    }
    pub(crate) fn write_header(&mut self) {
        self.octx.set_metadata(self.ictx.metadata().to_owned());
        // format::context::output::dump(&octx, 0, Some(&output_file));
        self.octx.write_header().unwrap();
        for (ost_index, _) in self.octx.streams().enumerate() {
            self.out_str_time_bases[ost_index] =
                self.octx.stream(ost_index as _).unwrap().time_base();
        }
    }
    fn finalize(&mut self) {
        self.octx.write_trailer().unwrap();
    }

    fn reencode_between_timestamps(
        &mut self,
        start: f64,
        end: f64,
        copy: bool,
    ) -> Result<(), Box<dyn std::error::Error>> {
        self.ictx
            .seek(start as i64, ..(start as i64))
            .expect("Failed to seek");

        let mut first_dts: Vec<Option<i64>> = vec![None; self.ictx.streams().len()];
        for (stream, mut packet) in self.ictx.packets() {
            let ist_index = stream.index();
            let ost_index = self.stream_mapping[ist_index];
            if ost_index < 0 {
                continue;
            }
            let tb = stream.time_base();

            let pts = packet
                .pts()
                .or_else(|| packet.dts())
                .ok_or(ffmpeg::Error::InvalidData)?;

            let time = pts as f64 * f64::from(tb);

            if time < start {
                continue;
            }
            if time > end {
                break;
            }

            // Инициализация first_dts
            let base = first_dts[ist_index].get_or_insert(packet.dts().unwrap_or(0));


            // Сдвигаем timestamps
            if let Some(pts) = packet.pts() {
                let shifted_pts = pts - *base;
                packet.set_pts(Some(shifted_pts));
                self.current_pts[ist_index] = shifted_pts;
                dbg!(self.current_pts[ist_index]);
            }
            if let Some(dts) = packet.dts() {
                let shifted_dts = dts - *base;
                packet.set_dts(Some(shifted_dts));
                self.current_dts[ist_index] = shifted_dts;
                dbg!(self.current_dts[ist_index]);
            }
            let ost_time_base = self.out_str_time_bases[ost_index as usize];
            match self.transcoders.get_mut(&ist_index) {
                Some(transcoder) => {
                    transcoder.send_packet_to_decoder(&packet);
                    transcoder.receive_and_process_decoded_frames(&mut self.octx, ost_time_base);
                }
                None => {
                    // Do stream copy on non-video streams.
                    packet.rescale_ts(self.in_str_time_bases[ist_index], ost_time_base);
                    packet.set_position(-1);
                    packet.set_stream(ost_index as _);
                    packet.write_interleaved(&mut self.octx).unwrap();
                }
            }
        }

        // Flush encoders and decoders.
        if copy {
            return Ok(());
        }
        for (ost_index, transcoder) in self.transcoders.iter_mut() {
            let ost_time_base = self.out_str_time_bases[*ost_index];
            transcoder.send_eof_to_decoder();
            transcoder.receive_and_process_decoded_frames(&mut self.octx, ost_time_base);
            transcoder.send_eof_to_encoder();
            transcoder.receive_and_process_encoded_packets(&mut self.octx, ost_time_base);
        }
        self.transcoders.clear();
        Ok(())
    }
    fn find_closest_keyframe_inside_boundaries(
        &mut self,
        target_time_seconds: f64,
        direction: Direction,
    ) -> Result<Option<f64>, Error> {
        let stream = self
            .ictx
            .streams()
            .best(Type::Video)
            .ok_or(ffmpeg::Error::StreamNotFound)?;

        let time_base = stream.time_base();
        let stream_index = stream.index();

        // Convert target time to stream time base units
        let target_ts = (target_time_seconds / f64::from(time_base)) as i64;

        let direction = match direction {
            Direction::Forward => ffmpeg::ffi::AVSEEK_FLAG_FRAME,
            Direction::Backward => ffmpeg::ffi::AVSEEK_FLAG_BACKWARD,
        };
        // Seek to nearest keyframe BEFORE or AT target_ts
        unsafe {
            ffmpeg::ffi::av_seek_frame(
                self.ictx.as_mut_ptr(),
                stream_index as i32,
                target_ts,
                direction,
            );
        }

        // Read packets forward until we find the first keyframe
        for (stream, packet) in self.ictx.packets() {
            if stream.index() != stream_index {
                continue;
            }

            if packet.is_key() {
                let ts = packet.pts().or_else(|| packet.dts()).unwrap();
                let keyframe_time = (ts as f64) * f64::from(time_base);
                return Ok(Some(keyframe_time));
            }
        }

        unsafe {
            ffmpeg::ffi::avformat_flush(self.ictx.as_mut_ptr());
        }
        Ok(None)
    }
    /// Fills first_kf and last_kf during initialization
    pub fn seek(&mut self) -> Result<(), Error> {
        self.first_kf =
            self.find_closest_keyframe_inside_boundaries(self.start, Direction::Forward)?;
        if self.first_kf.is_none() {
            // that means we don't have keyframes in given range at all
            // both are ok to be left as None
            return Ok(());
        }
        if let Some(last_kf) =
            self.find_closest_keyframe_inside_boundaries(self.end, Direction::Backward)?
        {
            // unwrap is safe because the value is checked above
            if last_kf != self.first_kf.unwrap() {
                self.last_kf = Some(last_kf)
            }
        }
        Ok(())
    }

    /// Copies packets between first and last keyframe, that's the lossless part
    pub fn copy_packets_between_keyframes(&mut self) -> Result<(), ffmpeg::Error> {
        assert!(
            self.first_kf.is_some() && self.last_kf.is_some(),
            "I can't do that without both first_kf and last_kf, man"
        );

        let start = self.first_kf.unwrap();
        let end = self.last_kf.unwrap();

        self.ictx
            .seek(self.first_kf.unwrap() as i64, (start as i64)..(end as i64))
            .expect("Failed to seek");

        // Запоминаем стартовые DTS для каждого стрима
        let mut first_dts: Vec<Option<i64>> = vec![None; self.ictx.streams().len()];

        for (stream, mut packet) in self.ictx.packets() {
            let istream_index = stream.index();

            // Стримы, которых нет в output — пропускаем
            if istream_index >= self.stream_mapping.len() {
                continue;
            }

            let tb = stream.time_base();

            let pts = packet
                .pts()
                .or_else(|| packet.dts())
                .ok_or(ffmpeg::Error::InvalidData)?;

            eprintln!("Packet pts: {}", pts);
            let time = pts as f64 * f64::from(tb);
            eprintln!("Stream timebase: {}", f64::from(tb));
            eprintln!("Packet time: {}", time);

            if time < start {
                continue;
            }
            if time > end {
                break;
            }

            // Инициализация first_dts
            let base = first_dts[istream_index].get_or_insert(packet.dts().unwrap_or(0));

            // Сдвигаем timestamps
            if let Some(pts) = packet.pts() {
                let new_pts = pts - *base + self.current_pts[istream_index as usize];
                packet.set_pts(Some(new_pts));
                dbg!(new_pts);
            }
            if let Some(dts) = packet.dts() {
                let new_dts = dts - *base + self.current_dts[istream_index as usize];
                packet.set_dts(Some(new_dts));
                dbg!(new_dts);
            }

            // Remap stream index
            packet.set_stream(self.stream_mapping[istream_index as usize] as usize);
            packet.write_interleaved(&mut self.octx).unwrap();
        }

        Ok(())
    }
}
enum Direction {
    Forward,
    Backward,
}
impl std::fmt::Debug for Saw2 {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "Streams: \n")?;
        for stream in self.ictx.streams() {
            let info = format!("{:?}", stream.parameters().medium());
            let id = format!("{:?}", stream.parameters().id());
            write!(f, "\nType: {{")?;
            write!(f, " {}", info)?;
            write!(f, "  {} ", id)?;
            write!(f, "}}\n")?;
        }
        // Write raw multiline string
        writeln!(f, "  start: {:?},", &self.start)?;
        writeln!(f, "  first_kf: {:?},", &self.first_kf)?;
        writeln!(f, "  last_kf: {:?},", &self.last_kf)?;
        writeln!(f, "  end: {:?},", &self.end)
    }
}
