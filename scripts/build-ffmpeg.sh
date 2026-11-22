#!/usr/bin/env bash
set -euo pipefail

export LC_ALL=C
root="$(cd "$(dirname "$0")/.." && pwd)"
runtime="$root/src-tauri/runtime"
work="$root/src-tauri/target/ffmpeg"
sources="$work/src"
downloads="$work/downloads"
prefix="$work/prefix"
make_command="$(command -v make)"

if [[ "$(gcc -dumpmachine)" != x86_64-w64-mingw32 ]]; then
  printf '%s\n' 'This build requires the Windows x64 MinGW-w64 GCC toolchain.' >&2
  exit 1
fi
mkdir -p "$sources" "$downloads" "$work/ffmpeg-build"

fetch_source() {
  local filename="$1" sha256="$2" url="$3" marker="$4"
  local archive="$downloads/$filename"

  if [[ ! -f "$archive" ]]; then
    curl --fail --location --silent --show-error --retry 2 --max-time 120 \
      "$url" -o "$archive.download"
    printf '%s  %s\n' "$sha256" "$archive.download" | sha256sum --check --status
    mv -f "$archive.download" "$archive"
  fi
  printf '%s  %s\n' "$sha256" "$archive" | sha256sum --check
  if [[ ! -f "$sources/$marker" ]]; then
    (cd "$sources"; cmake -E tar xf "$archive")
  fi
}

fetch_source ffmpeg-github.tar.gz e3963a50831c985933e1a625ed566ec4c7adb5c012c34fa9f84438e1d61bdacc \
  https://codeload.github.com/FFmpeg/FFmpeg/tar.gz/refs/tags/n7.1.5 FFmpeg-n7.1.5/configure
fetch_source libogg-1.3.6.tar.xz 5c8253428e181840cd20d41f3ca16557a9cc04bad4a3d04cce84808677fa1061 \
  https://downloads.xiph.org/releases/ogg/libogg-1.3.6.tar.xz libogg-1.3.6/CMakeLists.txt
fetch_source libvorbis-1.3.7.tar.xz b33cc4934322bcbf6efcbacf49e3ca01aadbea4114ec9589d1b1e9d20f72954b \
  https://downloads.xiph.org/releases/vorbis/libvorbis-1.3.7.tar.xz libvorbis-1.3.7/CMakeLists.txt

prefix_windows="$(cygpath -m "$prefix")"
for library in libogg-1.3.6 libvorbis-1.3.7; do
  cmake -S "$sources/$library" -B "$work/$library-build" -G Ninja \
    -DCMAKE_C_COMPILER=gcc -DCMAKE_BUILD_TYPE=Release -DBUILD_SHARED_LIBS=OFF \
    -DBUILD_TESTING=OFF -DCMAKE_INSTALL_PREFIX="$prefix_windows" \
    -DCMAKE_PREFIX_PATH="$prefix_windows" -DCMAKE_POLICY_VERSION_MINIMUM=3.5 \
    '-DCMAKE_C_FLAGS_RELEASE=-Os -DNDEBUG'
  cmake --build "$work/$library-build" --target install
done

export PKG_CONFIG_PATH="$prefix_windows/lib/pkgconfig"
export PKG_CONFIG_LIBDIR="$PKG_CONFIG_PATH"
pkg-config --print-errors --exists vorbis vorbisenc
cd "$work/ffmpeg-build"
# Keep local audio transcoding and the containers and protocols used for yt-dlp HLS downloads.
bash "$sources/FFmpeg-n7.1.5/configure" \
  --target-os=mingw32 --arch=x86_64 --enable-small --extra-version=beatmaps-starter-audio \
  --disable-autodetect --disable-everything --disable-doc --disable-debug \
  --disable-ffplay --disable-avdevice --disable-swscale --disable-postproc \
  --enable-schannel --enable-libvorbis --pkg-config-flags=--static --extra-ldflags=-static \
  --enable-protocol=file,pipe,http,https,tcp,tls,crypto \
  --enable-demuxer=aac,ac3,eac3,flac,hls,matroska,mov,mp3,mpegts,ogg,wav \
  --enable-muxer=adts,flac,ipod,matroska,mp4,mpegts,ogg,wav,webm \
  --enable-parser=aac,aac_latm,ac3,flac,h264,mpegaudio,opus,vorbis \
  --enable-decoder=aac,aac_latm,ac3,alac,eac3,flac,mp3,mp3float,opus,vorbis,pcm_s16le,pcm_s24le,pcm_s32le,pcm_f32le,pcm_f64le \
  --enable-encoder=flac,libvorbis,pcm_f32le,pcm_f64le,pcm_s32le --enable-filter=adelay,aresample,asetpts,atrim,ebur128 \
  --enable-bsf=aac_adtstoasc,h264_mp4toannexb
"$make_command" -r -j "${NUMBER_OF_PROCESSORS:-4}" ffmpeg.exe ffprobe.exe

ffmpeg="$work/ffmpeg-build/ffmpeg.exe"
smoke_wav="$work/smoke.wav"
sample_rate=44100
channels=2
bits_per_sample=16
smoke_data_size=$((sample_rate * channels * bits_per_sample / 8))

write_le16() {
  local value="$1" low high
  printf -v low '%02x' "$((value & 0xff))"
  printf -v high '%02x' "$(((value >> 8) & 0xff))"
  printf '%b' "\\x${low}\\x${high}"
}

write_le32() {
  local value="$1" byte0 byte1 byte2 byte3
  printf -v byte0 '%02x' "$((value & 0xff))"
  printf -v byte1 '%02x' "$(((value >> 8) & 0xff))"
  printf -v byte2 '%02x' "$(((value >> 16) & 0xff))"
  printf -v byte3 '%02x' "$(((value >> 24) & 0xff))"
  printf '%b' "\\x${byte0}\\x${byte1}\\x${byte2}\\x${byte3}"
}

{
  printf 'RIFF'
  write_le32 $((36 + smoke_data_size))
  printf 'WAVEfmt '
  write_le32 16
  write_le16 1
  write_le16 "$channels"
  write_le32 "$sample_rate"
  write_le32 $((sample_rate * channels * bits_per_sample / 8))
  write_le16 $((channels * bits_per_sample / 8))
  write_le16 "$bits_per_sample"
  printf 'data'
  write_le32 "$smoke_data_size"
} > "$smoke_wav"
dd if=/dev/zero bs="$smoke_data_size" count=1 status=none >> "$smoke_wav"

"$ffmpeg" -v error -nostdin -y \
  -i "$smoke_wav" -t 1 -vn \
  -af ebur128=framelog=verbose -ar 44100 -ac 2 -c:a libvorbis "$work/smoke.ogg"

mkdir -p "$runtime/licenses"
cp "$sources/FFmpeg-n7.1.5/COPYING.LGPLv2.1" "$runtime/licenses/FFmpeg-LICENSE.txt"
cp "$sources/libogg-1.3.6/COPYING" "$runtime/licenses/libogg-LICENSE.txt"
cp "$sources/libvorbis-1.3.7/COPYING" "$runtime/licenses/libvorbis-LICENSE.txt"
{ "$ffmpeg" -version; "$ffmpeg" -L; } > "$runtime/licenses/FFmpeg-build.txt"
cp "$ffmpeg" "$runtime/ffmpeg.exe.new"
mv -f "$runtime/ffmpeg.exe.new" "$runtime/ffmpeg.exe"
cp "$work/ffmpeg-build/ffprobe.exe" "$runtime/ffprobe.exe"
ls -lh "$runtime/ffmpeg.exe"
