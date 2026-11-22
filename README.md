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

## Windows 安装包

打包使用项目的音频版 FFmpeg。在 MSYS2 中准备 MinGW-w64 GCC、MSYS GNU Make、CMake、Ninja、NASM 和 pkg-config，然后运行 `bash scripts/build-ffmpeg.sh`。脚本构建 `src-tauri/runtime/ffmpeg.exe` 和 `ffprobe.exe`，并用谱面样例检查 Vorbis 导出。

执行 `npm run package:windows` 时，脚本会将网易云助手的运行依赖合并为脚本和数据文件，再和音频版 FFmpeg、FFprobe、本机 Node.js 以及校验后的 yt-dlp 一起打包。可以用 `BEATMAPS_FFMPEG` 和 `BEATMAPS_FFPROBE` 指定独立版本。安装包输出在 `src-tauri/target/release/bundle/nsis`。

自动更新使用 `https://github.com/hikarin31768/beatmaps-starter/releases/latest/download/latest.json`，GitHub 会将 `releases/latest` 指向最新的正式版。推送普通提交仍会生成预发布版本；发布正式版时，将与 `src-tauri/tauri.conf.json` 中版本一致的 `v<版本号>` 标签推送到 GitHub。工作流会签署安装包并生成 `latest.json`。GitHub Actions 需要配置 `TAURI_SIGNING_PRIVATE_KEY` 仓库 Secret；对应公钥已随应用配置。 [Tauri 更新发布说明](https://v2.tauri.app/plugin/updater/)

界面沿用了 Tauri Vue Template 的窗口与导航结构。 [Tauri Vue Template](https://github.com/Ming-D-W/Tauri-Vue-Template) · [MIT 许可](TEMPLATE-LICENSE)

平台接口和文件格式参考了这些项目的文档。 [网易云音乐 API Enhanced](https://neteasecloudmusicapienhanced.js.org/) · [Bilibili 接口资料](https://github.com/pskdje/bilibili-API-collect) · [yt-dlp](https://github.com/yt-dlp/yt-dlp) · [FFmpeg](https://ffmpeg.org/) · [Beat Saber 谱面格式文档](https://bsmg.wiki/mapping/map-format.html)

三家平台的品牌图标随应用打包。 [Iconify Simple Icons](https://icon-sets.iconify.design/simple-icons/)

BPM 检测参考 ArrowVortex 的起音、周期评分和节拍相位算法，使用 aubio 的 Complex 起音检测。 [ArrowVortex 源码](https://github.com/uvcat7/ArrowVortex/tree/5b63ff72ac8e6ef6af12801ebe042acdc479324f/src/Editor) · [aubio 许可证](https://github.com/aubio/aubio/blob/0.4.9/COPYING)
