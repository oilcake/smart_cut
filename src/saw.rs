use crate::codec::{AudioCodec, StreamCodec, VideoCodec};
use std::usize;

use ffmpeg_next::{
    self as ffmpeg,
    format::{self},
    media::Type,
    util::range::Range,
    Error, Rescale,
};

enum Direction {
    Forward,
    Backward,
}
struct Fragment {
    start: i64,
    end: i64,
}
impl Range<i64> for Fragment {
    fn start(&self) -> Option<&i64> {
        Some(&self.start)
    }

    fn end(&self) -> Option<&i64> {
        Some(&self.end)
    }
}

pub struct Saw {
    ictx: format::context::Input,
    octx: format::context::Output,
    stream_map: Vec<usize>,
    pub start: f64,
    pub first_kf: Option<f64>,
    pub last_kf: Option<f64>,
    end: f64,
    codecs: Vec<Option<StreamCodec>>,
}

impl std::fmt::Debug for Saw {
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

impl Saw {
    pub fn new(input: &str, output: &str, start: f64, end: f64) -> Result<Self, Error> {
        let ictx = format::input(&input)?;
        let mut octx = format::output(&output)?;
        let mut stream_map = Vec::with_capacity(ictx.nb_streams() as usize);
        let mut codecs: Vec<Option<StreamCodec>> = Vec::with_capacity(ictx.nb_streams() as usize);
        for istream in ictx.streams() {
            let codec_id = istream.parameters().id();
            let codec = ffmpeg::codec::decoder::find(codec_id).unwrap();
            let idx = istream.index();
            let params = istream.parameters();
            // set output stream
            let mut ostream = octx.add_stream(codec)?;
            ostream.set_parameters(params.clone());
            stream_map.insert(idx, ostream.index() as usize);
            // find decoder
            let decoder_ctx = ffmpeg::codec::context::Context::from_parameters(params.clone())?;
            let decoder = decoder_ctx.decoder();
            // and timebase
            let tb = istream.time_base();

            match decoder.medium() {
                ffmpeg::media::Type::Video => {
                    let dec = decoder.video()?;

                    let mut enc_ctx = ffmpeg::codec::context::Context::new();
                    enc_ctx.set_parameters(params)?;
                    let enc = enc_ctx.encoder().video()?;

                    let stream_codec = Some(StreamCodec::Video(VideoCodec {
                        decoder: dec,
                        encoder: enc,
                        in_time_base: tb,
                        out_time_base: tb,
                    }));
                    codecs.insert(idx, stream_codec);
                }

                ffmpeg::media::Type::Audio => {
                    let dec = decoder.audio()?;

                    let mut enc_ctx = ffmpeg::codec::context::Context::new();
                    enc_ctx.set_parameters(params)?;
                    let enc = enc_ctx.encoder().audio()?;

                    let stream_codec = Some(StreamCodec::Audio(AudioCodec {
                        decoder: dec,
                        encoder: enc,
                        in_time_base: tb,
                        out_time_base: tb,
                    }));
                    codecs.insert(idx, stream_codec);
                }

                _ => {
                    let stream_codec = Some(StreamCodec::Other);
                    codecs.insert(idx, stream_codec);
                }
            }
        }
        octx.write_header()?;
        Ok(Saw {
            ictx,
            octx,
            stream_map,
            start,
            first_kf: None,
            last_kf: None,
            end,
            codecs,
        })
    }

    /// Main function that does everything
    pub fn saw(&mut self) -> Result<(), Box<dyn std::error::Error>> {
        if let Some(first_kf) = self.first_kf {
            self.reencode_between_timestamps(self.start, first_kf)
                .unwrap();
        }
        if self.first_kf.is_some() && self.last_kf.is_some() {
            self.copy_packets_between_keyframes()?;
        }
        if let Some(last_kf) = self.last_kf {
            self.reencode_between_timestamps(last_kf, self.end).unwrap();
        }
        self.octx.write_trailer()?;
        Ok(())
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

    /// Does actual work in keyframe seeking
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

    /// Copies packets between first and last keyframe, that's the lossless part
    pub fn copy_packets_between_keyframes(&mut self) -> Result<(), ffmpeg::Error> {
        assert!(
            self.first_kf.is_some() && self.last_kf.is_some(),
            "I can't do that without both first_kf and last_kf, man"
        );

        let start = self.first_kf.unwrap();
        let end = self.last_kf.unwrap();

        let fragment = Fragment {
            start: self.first_kf.unwrap() as i64,
            end: self.last_kf.unwrap() as i64,
        };
        self.ictx
            .seek(self.first_kf.unwrap() as i64, fragment)
            .expect("Failed to seek");

        // Запоминаем стартовые DTS для каждого стрима
        let mut first_dts: Vec<Option<i64>> = vec![None; self.ictx.streams().len()];

        for (stream, mut packet) in self.ictx.packets() {
            let istream_index = stream.index();

            // Стримы, которых нет в output — пропускаем
            if istream_index >= self.stream_map.len() {
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
            let base = first_dts[istream_index].get_or_insert(packet.dts().unwrap_or(0));

            // Сдвигаем timestamps
            if let Some(pts) = packet.pts() {
                packet.set_pts(Some(pts - *base));
            }
            if let Some(dts) = packet.dts() {
                packet.set_dts(Some(dts - *base));
            }

            // Remap stream index
            packet.set_stream(self.stream_map[istream_index]);
            packet.write_interleaved(&mut self.octx).unwrap();
        }

        Ok(())
    }

    /// Reencodes everything else, that does not fall between first and last keyframe
    fn reencode_between_timestamps(
        &mut self,
        start: f64,
        end: f64,
    ) -> Result<(), Box<dyn std::error::Error>> {
        println!("reencode_between_timestamps");

        for (stream, packet) in self.ictx.packets() {
            let stream_id = stream.index();
            let out_stream_id = self.stream_map[stream_id];

            let Some(codec) = &mut self.codecs[stream_id] else {
                continue;
            };

            match codec {
                StreamCodec::Video(video_codec) => {
                    process_video_packet(
                        video_codec,
                        start,
                        end,
                        &packet,
                        out_stream_id,
                        &mut self.octx,
                    )
                    .unwrap();
                }

                StreamCodec::Audio(audio_codec) => {
                    process_audio_packet(
                        audio_codec,
                        start,
                        end,
                        &packet,
                        out_stream_id,
                        &mut self.octx,
                    )
                    .unwrap();
                }

                StreamCodec::Other => {
                    // можно remux без реэнкода
                    packet.write_interleaved(&mut self.octx).unwrap();
                }
            }
        }
        Ok(())
    }
}
// rescale helper
fn seconds_to_pts(sec: f64, tb: ffmpeg::Rational) -> i64 {
    let ftb: f64 = tb.into();
    (sec / ftb).round() as i64
}

fn process_video_packet(
    video_codec: &mut VideoCodec,
    start: f64,
    end: f64,
    packet: &ffmpeg::Packet,
    out_stream_index: usize,
    octx: &mut ffmpeg::format::context::Output,
) -> Result<(), Box<dyn std::error::Error>> {
    let start_pts = seconds_to_pts(start, video_codec.in_time_base);
    let end_pts = seconds_to_pts(end, video_codec.in_time_base);

    video_codec.decoder.send_packet(packet)?;

    let mut frame = ffmpeg::frame::Video::empty();
    while video_codec.decoder.receive_frame(&mut frame).is_ok() {
        let Some(pts) = frame.pts() else {
            continue;
        };

        if pts < start_pts {
            continue;
        }

        if pts > end_pts {
            break;
        }

        let new_pts = Some(pts.rescale(video_codec.in_time_base, video_codec.out_time_base));
        frame.set_pts(new_pts);

        video_codec.encoder.send_frame(&frame)?;

        let mut out = ffmpeg::Packet::empty();
        while video_codec.encoder.receive_packet(&mut out).is_ok() {
            out.set_stream(out_stream_index);
            out.write_interleaved(octx)?;
        }
    }
    Ok(())
}
fn process_audio_packet(
    audio_codec: &mut AudioCodec,
    start: f64,
    end: f64,
    packet: &ffmpeg::Packet,
    out_stream_index: usize,
    octx: &mut ffmpeg::format::context::Output,
) -> Result<(), Box<dyn std::error::Error>> {
    let start_pts = seconds_to_pts(start, audio_codec.in_time_base);
    let end_pts = seconds_to_pts(end, audio_codec.in_time_base);

    audio_codec.decoder.send_packet(packet)?;

    let mut frame = ffmpeg::frame::Audio::empty();
    while audio_codec.decoder.receive_frame(&mut frame).is_ok() {
        let Some(pts) = frame.pts() else {
            continue;
        };

        let frame_end = pts + frame.samples() as i64;

        if frame_end < start_pts {
            continue;
        }

        if pts > end_pts {
            break;
        }

        let new_pts = Some(pts.rescale(audio_codec.in_time_base, audio_codec.out_time_base));
        frame.set_pts(new_pts);

        audio_codec.encoder.send_frame(&frame)?;

        let mut out = ffmpeg::Packet::empty();
        while audio_codec.encoder.receive_packet(&mut out).is_ok() {
            out.set_stream(out_stream_index);
            out.write_interleaved(octx)?;
        }
    }
    Ok(())
}
