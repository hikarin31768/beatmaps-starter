# Beatmaps Starter

将网易云音乐、Bilibili、YouTube 的单首歌曲或单个视频整理成 Beat Saber 的可编辑谱面。支持试听、自动检测 BPM、对齐音频偏移和替换封面。

## 使用

1. 首次启动时选择包含 `Beat Saber.exe` 的目录，按需要配置平台账号，再设定默认谱面版本、谱师和音频偏移。
2. 在首页粘贴链接。解析后点击曲名可修改曲名、歌手和副标题，点击封面可更换图片。
3. 等待自动填入 BPM，按需手动修正。试听使用最低可用音质，圆点控制全曲播放位置，两个倒三角决定谱面预览区间。「最小变动」仅添加对齐首拍所需的留白，「按固定值」会添加或裁剪留白，使它尽量接近设置值（默认 2 秒）。
4. 选择导出音质，点击「导出谱面」。应用会写入 `Beat Saber_Data\CustomWIPLevels`，同名导出会自动使用带编号的目录。谱面音频统一保存为 `song.ogg`，有损和无损输入均受 10 MB 上限约束。已有 Vorbis 音轨在无需调整留白时直接封装，无损输入优先使用最高质量的 Vorbis 编码，其他有损音源以输入比特率为编码目标。超过 10 MB 时降低码率并重编码，保留输入采样率。封面保存为质量 100 的方形 `cover.jpg`，边长大于 512 像素，文件不超过 1.5 MB。

V2/V3 写入 `BPMInfo.dat`，可在编辑器里创建难度。V4 将节拍数据写入 `AudioData.dat`，并生成一个空白的 Standard Expert+ 难度，将谱师署名写入该难度。两种格式均使用导出音频的实际采样率、样本数和最终 BPM。

网易云和 Bilibili 支持扫码登录。网易云也可填写账号密码或使用手机短信，Bilibili 可以在官方登录窗口完成密码、短信及验证码验证。YouTube 使用浏览器登录后导出的 Netscape Cookie。应用只导入 YouTube 域的 Cookie，Windows 使用当前用户的 DPAPI 加密保存各平台会话。参考 [yt-dlp 的 YouTube 登录说明](https://github.com/yt-dlp/yt-dlp/wiki/Extractors#exporting-youtube-cookies)。

## 本地开发

需要 Windows x64、Node.js 24+、Rust MSVC、Visual C++ Build Tools 和 WebView2。开发时让 `ffmpeg`（包含 `libvorbis`）、`ffprobe` 和 `yt-dlp` 可通过 PATH 找到，或者把它们的可执行文件放入 `src-tauri/runtime`。

```powershell
npm ci
npm --prefix src-tauri/runtime ci --ignore-scripts
npm run tauri dev
```

前端构建使用 `npm run build`。开发时的设置和平台会话保存在 `src-tauri/.dev-data`。

## 引用声明

界面沿用了 Tauri Vue Template 的窗口与导航结构。 [Tauri Vue Template](https://github.com/Ming-D-W/Tauri-Vue-Template) · [MIT 许可](#tauri-vue-template-mit-license)

平台接口和文件格式参考了这些项目的文档。 [网易云音乐 API Enhanced](https://neteasecloudmusicapienhanced.js.org/) · [Bilibili 接口资料](https://github.com/pskdje/bilibili-API-collect) · [yt-dlp](https://github.com/yt-dlp/yt-dlp) · [FFmpeg](https://ffmpeg.org/) · [Beat Saber 谱面格式文档](https://bsmg.wiki/mapping/map-format.html)

三家平台的品牌图标随应用打包。 [Iconify Simple Icons](https://icon-sets.iconify.design/simple-icons/)

BPM 检测参考 ArrowVortex 的起音、周期评分和节拍相位算法，使用 aubio 的 Complex 起音检测。 [ArrowVortex 源码](https://github.com/uvcat7/ArrowVortex/tree/5b63ff72ac8e6ef6af12801ebe042acdc479324f/src/Editor) · [aubio 许可证](https://github.com/aubio/aubio/blob/0.4.9/COPYING)

## License
The application source is licensed under GPL-3.0-or-later (see `LICENSE`). The bundled FFmpeg build is configured without `--enable-gpl`, so FFmpeg and its bundled libraries retain their respective upstream licenses; packaged builds include those notices under `src-tauri/runtime/licenses`.

### Tauri Vue Template MIT License

```text
MIT License

Copyright (c) 2024 [Your Name]

Permission is hereby granted, free of charge, to any person obtaining a copy
of this software and associated documentation files (the "Software"), to deal
in the Software without restriction, including without limitation the rights
to use, copy, modify, merge, publish, distribute, sublicense, and/or sell
copies of the Software, and to permit persons to whom the Software is
furnished to do so, subject to the following conditions:

The above copyright notice and this permission notice shall be included in all
copies or substantial portions of the Software.

THE SOFTWARE IS PROVIDED "AS IS", WITHOUT WARRANTY OF ANY KIND, EXPRESS OR
IMPLIED, INCLUDING BUT NOT LIMITED TO THE WARRANTIES OF MERCHANTABILITY,
FITNESS FOR A PARTICULAR PURPOSE AND NONINFRINGEMENT. IN NO EVENT SHALL THE
AUTHORS OR COPYRIGHT HOLDERS BE LIABLE FOR ANY CLAIM, DAMAGES OR OTHER
LIABILITY, WHETHER IN AN ACTION OF CONTRACT, TORT OR OTHERWISE, ARISING FROM,
OUT OF OR IN CONNECTION WITH THE SOFTWARE OR THE USE OR OTHER DEALINGS IN THE
SOFTWARE.
```
