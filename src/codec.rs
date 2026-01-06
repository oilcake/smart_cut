use ffmpeg_next::{self as ffmpeg};

pub(crate) enum StreamCodec {
    Video {
        decoder: ffmpeg::decoder::Video,
        encoder: ffmpeg::encoder::video::Video,
        in_time_base: ffmpeg::Rational,
        out_time_base: ffmpeg::Rational,
    },
    Audio {
        decoder: ffmpeg::decoder::Audio,
        encoder: ffmpeg::encoder::audio::Audio,
        in_time_base: ffmpeg::Rational,
        out_time_base: ffmpeg::Rational,
    },
    Other, // subtitles / data
}
