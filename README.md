# DiscCraft

Pure Rust のオープンソース DVD / Blu-ray オーサリングソフト。Adobe Encore 相当の機能を目指す。

## 概要

DiscCraft は映像・音声・静止画から DVD-Video / Blu-ray（BDMV）を作るオーサリングソフト。メニュー、チャプター、複数の音声・字幕、ナビゲーションを組み立て、規格に合わせてトランスコードし、フォルダ・ISO イメージ・ディスクに書き出す。FilmCraft で編集した作品をディスクにする出口にもなる。

ゴール:

- Encore を使っていた人が迷わない画面構成と操作
- 家庭用プレーヤーで再生できる規格準拠の DVD-Video。次に Blu-ray（HDMV）
- Pure Rust、クリーンルーム実装、Windows / macOS / Linux
- すべての操作がコマンド。GUI、CLI、JSON 制御チャネル、MCP サーバーから同じ操作ができる
- クラッシュしない。作業を失わない

非ゴール:

- コピーガード（CSS、AACS など）。ライセンスが必要なため。リージョンコードのフラグ設定だけ対応する
- BD-J（Java）メニュー。Blu-ray は HDMV のみ
- DTS、Dolby TrueHD、DTS-HD のエンコード
- 動画編集。編集は FilmCraft に任せて連携する

## 状態

M0〜M7 は完了、M8（Blu-ray）と M9（PhotoCraft / SoundCraft 連携）は作業中、画面を Encore に寄せる UI パリティは M6.44 まで
進んでいる（ROADMAP.md、docs/ui-parity.md）。

- MP4 / MOV などの素材から、メニュー付きの DVD（`VIDEO_TS` フォルダ、ISO）と Blu-ray（BDMV フォルダ、ISO）を作れる。
  規格に合わない素材はビルド時に自動で変換する（出力先の `disccraft-cache` に保存）。
- GUI（`cargo run --release -p disccraft`）は Encore と同じ配置とメニューバー: プロジェクト / メニュー / タイムライン / ビルドのタブ、
  モニター・メニュービューア・フローチャート、プロパティ、レイヤー / ライブラリ、タイムラインビューア。
- メニュー: ボタンの配置（複数選択、整列、均等配置、アレンジ、コピーと貼り付け）、リンク（ピックウィップ）、初期設定ボタン、
  自動アクティベート、背景（色 / 画像 / 動画）とメニューの音声、エンドアクションとデュレーション、チャプターインデックス、ライブラリのプリセット。
- タイムライン: チャプターポイント、オーディオ / 字幕トラック、エンドアクション、リモートメニュー、再生（Space、映像のみ）。
- ナビゲーション: ファーストプレイ、タイトルボタン、スライドショー、再生リスト。ビルドしたディスクを自前の VM でプレビューできる
  （リモコンの十字キー、タイトル / メニューボタン）。
- すべての操作はエンジンのコマンドで、GUI、CLI、MCP から同じことができる。1 回の操作は 1 回で取り消せる。

## 試し方（Windows）

ffmpeg と VLC は比較用（出来上がったディスクを外のプレーヤーで確かめる、テスト素材を作る）で、DiscCraft 自体は使わない
（AGENTS.md ルール 9）。ffmpeg があればテスト素材を作れ、VLC で再生して確かめられる。

```
winget install Gyan.FFmpeg              # 未導入なら。新しいターミナルで ffmpeg が使える
cargo test -p disccraft-dvd             # ffmpeg で target/fixtures にテスト素材（20 秒の映像と AC-3）を作る
cargo run -p disccraft-cli -- run demo.dcproj docs/examples/menu-demo.jsonl
cargo run -p disccraft-cli -- inspect demo.dcproj
cargo run -p disccraft-cli -- build demo.dcproj --out out
cargo run -p disccraft-cli -- build demo.dcproj --iso demo.iso   # ISO イメージ（エクスプローラーでダブルクリックするとマウントできる）
```

VLC で「メディア」→「ディスクを開く」→「DVD」を選び、`out` フォルダを開く。メニューが出たら上下キーと Enter で操作する。

まとめて確かめるときは、リリースビルドと DVD / Blu-ray のデモ（フォルダと ISO）を一度に作るスクリプトを使う:

```
powershell -ExecutionPolicy Bypass -File scripts\build-demo.ps1
```

`target\demo` に GUI（`bin\disccraft.exe`）、CLI、各デモと `確認方法.txt` ができる。
デモは DVD（メニュー、M7 の全部入り、16:9）と Blu-ray（4:3、16:9）。16:9 のデモは中央の白い枠が正方形に見えれば縦横比が正しい。
確認した結果は docs/hardware-compat.md に記録する。

スライドショー、プレイリスト、モーションメニューのデモ（素材は `cargo test -p disccraft-dvd` と ffmpeg で作る）:

```
ffmpeg -i target/fixtures/h264_aac.mp4 -vf fps=1 -frames:v 3 target/fixtures/slide%d.png
cargo run -p disccraft-cli -- run m7.dcproj docs/examples/m7-demo.jsonl
cargo run -p disccraft-cli -- build m7.dcproj --out out7
```

## 機能

できること:

- DVD: MPEG-2 / LPCM / AC-3 のパススルーと MPEG-2 への再エンコード、VIDEO_TS フォルダと ISO イメージ（2 層のレイヤーブレイク）
- DVD: タイトル、チャプター、複数の音声トラック、SRT 字幕、16:9、リージョンコード
- Blu-ray: BDMV フォルダと ISO イメージ、PG 字幕、IG メニュー（HDMV）
- メニュー: 静止メニュー、ボタンとハイライト、モーションメニュー、タイムアウト（デュレーションとエンドアクション）、
  自動アクティベート、PSD 読み込みとレイヤー名によるボタン認識、チャプターインデックス
- 素材: 映像（MP4 / MOV / MKV / m2v など）、音声（WAV / AC-3 / MP3 / FLAC など）、画像（PNG / JPEG / PSD / TIFF）、
  ベクター画像（SVG。表示する大きさで描くのでぼけない）、字幕（SRT / WebVTT / タイムコード付きテキスト。UTF-8 / UTF-16 / Shift_JIS）
- ナビゲーション: ファーストプレイ、タイトルボタン、リモートメニュー、リンク、フローチャート、プレイリスト、スライドショー
- トランスコード: 自動トランスコード、容量に合わせたビットレート配分、キャッシュ
- 出力: DVD-5 / DVD-9、BD-25 / BD-50、BDXL（100 GB の 3 層、128 GB の片面 4 層）、容量メーター
- 検証: プロジェクトチェック、VM シミュレータとプレビュー、読み戻し検証

予定:

- ディスクへの書き込み、Blu-ray の HD（H.264）と ポップアップメニュー
- メニュー: カラーセット、ループポイント、Blu-ray の動くメニューとメニューの音声
- タイムライン: 再生時の音、上書き、チャプタープレイリスト
- トランスコード: 品質プリセット、2 パス、3:2 プルダウン

## 必要なもの

Rust stable 1.95 以上（rustup）。Windows を最優先、macOS / Linux も対象。

## 開発

```
cargo build
cargo xtask ci
cargo xtask fmt
cargo xtask layers
cargo xtask assets
```

`cargo xtask ci` は品質ゲートで、コミット前に必ず通す。詳細なルールは AGENTS.md を読む。

## AI エージェントから使う（MCP）

`disccraft-cli mcp` は MCP サーバー（標準入出力の JSON-RPC）として動く。ツールは `disccraft_commands`（コマンド一覧）、
`disccraft_execute`（エンジンのコマンドを実行）、`disccraft_inspect`（プロジェクトとチェック結果）の 3 つ。

```
claude mcp add disccraft -- C:\work\DiscCraft\target\release\disccraft-cli.exe mcp
```

## リポジトリ構成

```
DiscCraft/
├─ apps/
│  ├─ disccraft/
│  └─ disccraft-cli/
├─ crates/
│  ├─ mpeg2venc/
│  ├─ ac3enc/
│  ├─ psmux/
│  ├─ spu/
│  ├─ ifo/
│  ├─ m2tsmux/
│  ├─ bdmv/
│  ├─ pgs/
│  ├─ udf/
│  ├─ project/
│  ├─ vm/
│  ├─ menu/
│  ├─ motion/
│  ├─ photo/        （PhotoCraft 連携: PSD など）
│  ├─ sound/        （SoundCraft 連携）
│  ├─ nav/
│  ├─ transcode/
│  ├─ check/
│  ├─ dvd/
│  ├─ bd/
│  ├─ build/
│  ├─ engine/
│  ├─ ui-egui/
│  ├─ automation/
│  └─ platform/
├─ assets/
├─ docs/
└─ xtask/
```

## ドキュメント

- AGENTS.md（ルール）
- ROADMAP.md
- docs/architecture.md
- docs/testing.md
- docs/target-app-parity.md
- docs/ui-parity.md（Encore の画面との対応、コマンドの一覧）
- docs/hardware-compat.md（プレーヤーでの確認の記録）
- docs/adr/0001-initial-design.md（初期設計書）

## FilmCraft の再利用

FilmCraft (https://github.com/storytold/filmcraft, MIT OR Apache-2.0) の crate を git 依存（rev 固定）で使う。再利用した crate は NOTICE に記載する。

| crate | 使い道 |
|---|---|
| filmcraft-time | 整数 Tick による正確な時間 |
| filmcraft-geom | 座標と矩形 |
| filmcraft-color | 色変換 |
| filmcraft-bitstream | ビット読み書き |
| filmcraft-frame | フレーム表現 |
| filmcraft-media | 素材の読み込み |
| filmcraft-codecs | MP4 / MOV / MKV / MXF / MPEG-TS / PS などのデコード |
| filmcraft-mpeg2v | MPEG-2 エンコーダの検証と素材判定 |
| filmcraft-mpegts | 書き出した VOB / m2ts の読み戻し検証 |
| filmcraft-ac3 | AC-3 エンコーダの検証 |
| filmcraft-h264 | H.264 復号と検証 |
| filmcraft-h264enc | Blu-ray の映像エンコード |
| filmcraft-text | メニューの文字描画 |
| filmcraft-audio-dsp | ラウドネス測定と正規化 |

## 商標

「DVD」「Blu-ray Disc」は名称の説明的な使用のみで、ロゴは使わない。Adobe Encore は Adobe の商標で、DiscCraft は Adobe と関係がない。

## ライセンス

MIT OR Apache-2.0 のデュアルライセンス。詳細は LICENSE-MIT と LICENSE-APACHE を読む。明示的な合意がない限り、このリポジトリへの貢献も同じデュアルライセンスで提供される。
