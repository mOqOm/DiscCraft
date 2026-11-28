# DiscCraft

[English](README.md) | 日本語

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

開発中（0.1.0）。今できること:

- MP4 / MOV やビデオカメラの DV などの素材から、メニュー付きの DVD（`VIDEO_TS` フォルダ、ISO）と Blu-ray（BDMV フォルダ、ISO）を作れる。
  規格に合わない素材はビルド時に自動で変換する（既定では出力先の `disccraft-cache`。`build --cache <フォルダ>` で出力の外の
  フォルダに置いて共有できる）。
- GUI（`cargo run --release -p disccraft`）は Encore と同じ配置とメニューバー: プロジェクト / メニュー / タイムライン / ビルドのタブ、
  モニター・メニュービューア・フローチャート、プロパティ、レイヤー / ライブラリ、タイムラインビューア。タブはドラッグで並べ替え・
  移動・分割・フローティングでき、× で閉じてウィンドウメニューで開き直せる。並びはワークスペースとして名前を付けて保存できる。
  プロジェクトパネルの右クリックは Encore と同じ並び（未実装の行は灰色）。
- 何時間もある数十 GB の動画も、取り込んですぐ使える: ファイルは裏で開き、H.264 の mkv の長さと寸法はヘッダーから読む。
  モニターの絵は裏でデコードするので、再生ヘッドをドラッグしているあいだも画面は止まらない。
- メニュー: ボタンの配置（複数選択、整列、均等配置、アレンジ、コピーと貼り付け）、リンク（ピックウィップ）、初期設定ボタン、
  自動アクティベート、背景（色 / 画像 / 動画）とメニューの音声、エンドアクションとデュレーション、チャプターインデックス、
  ライブラリのプリセット（複数ページのチャプターのテンプレートも）と、PSD や画像のファイル・フォルダーからのライブラリの項目、
  画像とシェイプの層（ドロップシャドウ、反転、回転）、ガイド、縦書き文字、テキストサブピクチャの作成。
- タイムライン: 複数のビデオクリップをつないだ 1 本のタイトル（クリップは直後に詰めて置き、つなぎ目にチャプター。そのまま
  入る MPEG-2 のトリムは GOP の頭へ吸着。変換が要る素材も HD もつないでビルドでき、トリムはフレームの位置どおり）。チャプターポイント、
  オーディオ / 字幕トラック（字幕はタイムラインのカラーセットで色を選ぶ）、エンドアクション、リモートメニュー、
  再生（Space、音つき。聞くオーディオトラックは見出しのスピーカーで選ぶ。絵をデコードできない動画も音は再生し、
  モニターに理由を出す）。
- ナビゲーション: ファーストプレイ、タイトルボタン、スライドショー（スライドの名前と説明を字幕にできる）、再生リスト、
  チャプター再生リスト。ファイル ▸ プレビューで、ビルドせずにディスクを
  プロジェクトプレビューの窓で試せる（自前の VM。リモコンの十字キー、タイトル / メニュー、オーディオ / 字幕、チャプター、一時停止。
  タイトルは映像と音つきで再生し、終わるとエンドアクションへ。ビルドしたディスクもモニターでプレビューできる）。
- すべての操作はエンジンのコマンドで、GUI、CLI、MCP から同じことができる。1 回の操作は 1 回で取り消せる。

## 機能

できること:

- DVD: MPEG-2（720 / 704 / 352 幅）/ MPEG-1 / LPCM（96 kHz・24 ビットまで）/ AC-3 / DTS / MPEG-1 Layer II（PAL）のパススルーと
  MPEG-2 への再エンコード、VIDEO_TS フォルダと ISO イメージ（2 層のレイヤーブレイク）
- DVD: タイトル（タイムライン 1 本に複数のビデオクリップをつないだものを含む）、チャプター、複数の音声トラック、字幕
  （カラーセットとカラーグループ）、16:9、リージョンコード、プレイリストとチャプター再生リスト
- Blu-ray: BDMV フォルダと ISO イメージ、PG 字幕、IG メニュー（HDMV。常時表示 / ポップアップ、モーションメニュー）、
  ユーザー操作（UO マスク）。映像は SD（720×480 / 720×576）の MPEG-2 か、HD（1080p 23.976 / 24、
  720p 59.94 / 50 / 23.976 / 24 は H.264、1080i 59.94 / 50 は MPEG-2 か、ディスクのコーデックの設定でインターレースの H.264。タイトル、字幕、メニューとポップアップも HD の大きさ。
  29.97 / 25 fps の素材は 1080i 59.94 / 50 でそのままの速さの HD になる。`.m2ts` / `.mkv` / `.mp4` / `.mov` の Blu-ray に合った H.264 は、
  インターレースの 1080i も含めて、変換せずにそのまま収録する。mkv / mp4 / mov は元のファイルから直接ディスクへ書くので、
  映像の分の作業用の空きは要らない）。音声は LPCM（7.1・192 kHz・24 ビットまで）、AC-3、DTS をそのまま収録する
- メニュー: 静止メニュー、ボタンとハイライト、モーションメニュー、タイムアウト（デュレーションとエンドアクション）、
  自動アクティベート、PSD 読み込みとレイヤー名によるボタン認識、編集できる文字のレイヤー付きの PSD の書き出し、
  チャプターインデックス
- 素材: 映像（MP4 / MOV / MKV / m2v / DV など）、音声（WAV / AC-3 / MP3 / FLAC など）、画像（PNG / JPEG / PSD / TIFF）、
  ベクター画像（SVG。表示する大きさで描くのでぼけない）、字幕（SRT / WebVTT / タイムコード付きテキスト。UTF-8 / UTF-16 / Shift_JIS）
- ナビゲーション: ファーストプレイ、タイトルボタン、リモートメニュー、リンク、フローチャート、プレイリスト、スライドショー
- Blu-ray から DVD へ: ファイル ▸ 読み込み ▸ Blu-ray… で、暗号化されていない Blu-ray のプレイリストをタイムラインに（クリップ、チャプター、LPCM / AC-3 の音声）、メニューは同じボタンが同じチャプターにリンクしたモーションメニューに。そのまま DVD にビルドできる。コマンドラインの `bd-chapters` で Blu-ray のチャプターを出せる
- ディスクへの書き込み（Windows）: ビルドパネルの出力「DVD ディスク / Blu-ray ディスク」で、レコーダー・書き込み速度・コピー数を選んで書き込む（書き換え可能ディスクの消去も）。コマンドラインの `recorders` と `burn` も
- トランスコード: 自動トランスコード（音声は Dolby Digital（AC-3）192 kbps）、容量に合わせたビットレート配分、キャッシュ、
  複数の素材の変換を並列に（MPEG-2 エンコーダはマクロブロックの行でマルチスレッド、AC-3 は区間に分けて全コアで）。
  プロジェクトパネルの右クリックの「今すぐトランスコード」で、素材の変換をビルドの前に裏で済ませられる。Encore の品質プリセット
  （VBR 1 パス / 2 パス、CBR）と自分のプリセット、アセットごとのトランスコード設定とオリジナルに戻す、23.976 の素材の 3:2 プルダウン、
  Windows では HD の H.264 をハードウェアエンコーダーで（環境設定）
- 出力: DVD-5 / DVD-9、BD-25 / BD-50、BDXL（100 GB の 3 層、128 GB の片面 4 層）、容量メーター、DVD-ROM / BD-ROM コンテンツ（パソコン用のファイルを映像と一緒に入れる）
- 検証: プロジェクトチェック（結果は項目ごとにまとめ、Encore と同じ文言を出す）、VM シミュレータとプレビュー、読み戻し検証

## 必要なもの

Rust stable 1.95 以上（rustup）。Windows を最優先、macOS / Linux も対象。Linux では再生の音（cpal）のために
ALSA の開発用パッケージ（Debian / Ubuntu の `libasound2-dev`、Fedora の `alsa-lib-devel`）が要る。

リリースには、GitHub のランナーでビルドしたパッケージを置いている:

| システム | ファイル |
|---|---|
| Windows（x64、Arm64） | `.msi`（インストーラー）、`-portable.zip`（インストール不要） |
| macOS（Apple シリコンと Intel を 1 つに） | `.dmg`（DiscCraft.app）、`disccraft-cli-…-macos-universal.zip`（コマンドライン） |
| Linux（x86_64、aarch64） | `.AppImage`（更新用の `.zsync` つき）、`.flatpak`、`.deb`、`.rpm`、`.pkg.tar.zst`（Arch）、`.tar.gz` |
| Linux（riscv64） | `.deb`、`.rpm`、`.tar.gz` |

**動作を確認しているのは Windows（x64）のビルドだけ。** ほかは自動でビルドしてパッケージにしただけで、画面を出して使った
ことも、作ったディスクを確かめたこともない。開発者の証明書での署名はしていないので、Windows ではインストーラーの初回に
SmartScreen が確認を求め、macOS では初回に Gatekeeper に止められる（右クリックの「開く」か、
`xattr -d com.apple.quarantine` で外す）。Linux のパッケージには ALSA（`libasound2`）と、X11 か Wayland のライブラリが要る。
AppImage は最初の引数に `disccraft-cli` を付けるとコマンドラインが動く。動いた / 動かなかったの報告を歓迎する。

## ビルドと使い方

```
cargo build --release
```

`target/release` に GUI（`disccraft`）と CLI（`disccraft-cli`）ができる。

GUI は `disccraft` を起動し、ようこそ画面かファイルメニューから新規プロジェクトを作る。素材を読み込み、タイムラインと
メニューを作ってリンクし、ビルドパネルからフォルダか ISO イメージに書き出す。

CLI は、1 行に 1 つのコマンド（JSON）を順に実行してプロジェクトを作り、ビルドする:

```
disccraft-cli commands                           # コマンドの一覧
disccraft-cli run project.dcproj commands.jsonl  # コマンドを順に実行する（なければプロジェクトを作る）
disccraft-cli inspect project.dcproj             # プロジェクトとチェックの結果
disccraft-cli build project.dcproj --out out     # VIDEO_TS / BDMV フォルダに書き出す
disccraft-cli build project.dcproj --iso disc.iso
```

`commands.jsonl` の例（映像 1 本とメニュー 1 枚の DVD）:

```
{"command": "file.new", "params": {"kind": "dvd", "standard": "ntsc"}}
{"command": "asset.import", "params": {"path": "movie.mp4"}}
{"command": "timeline.create", "params": {"asset": 1, "name": "本編"}}
{"command": "menu.create", "params": {"name": "メイン"}}
{"command": "menu.addButton", "params": {"menu": 3, "label": "再生", "target": {"timeline": 2}}}
{"command": "timeline.setEndAction", "params": {"timeline": 2, "target": "lastMenu"}}
{"command": "nav.setFirstPlay", "params": {"target": {"menu": 3}}}
```

規格に合わない素材はビルドのときに変換し、出力先の `disccraft-cache` に残す（2 回目からは変換を飛ばす）。

開発の道具は `cargo xtask`（コマンドの一覧）。`cargo xtask ci` が品質ゲートで、整形の検査、clippy、テスト、crate の層、
素材のライセンス、マニュアルのリンクと画像を順に検査する。ffmpeg があれば、DiscCraft の出力を ffmpeg と比べるテストも動く
（なければその比較だけを飛ばす）。

使い方は [ユーザーマニュアル](docs/manual/ja/README.md) にある。まだできないことは [残りの課題と予定](docs/open-issues.ja.md) にまとめた。版ごとの変更は [変更履歴](CHANGELOG.ja.md) にある。

## AI エージェントから使う（MCP）

`disccraft-cli mcp` は MCP サーバー（標準入出力の JSON-RPC）として動く。ツールは `disccraft_commands`（コマンド一覧）、
`disccraft_execute`（エンジンのコマンドを実行）、`disccraft_inspect`（プロジェクトとチェック結果）の 3 つ。

MCP クライアントには、コマンドに `<リポジトリのパス>/target/release/disccraft-cli`、引数に `mcp` を登録する（標準入出力でつなぐ）。

## 構成

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
└─ xtask/
```

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
| filmcraft-text | メニューの文字描画 |
| filmcraft-audio-dsp | ラウドネス測定と正規化 |
| filmcraft-project、filmcraft-format | FilmCraft のプロジェクト（`.fcproj`）の読み込み: シーケンスのマーカーをチャプターポイントにする |

FilmCraft の crate は craft-libs（https://github.com/storytold/craft-libs、MIT OR Apache-2.0）の crate（映像・音声のデコードなど）を使う。DiscCraft は craft-libs の craft-audio（AC-3 エンコーダ。変換した音声を Dolby Digital にする）、craft-video（Blu-ray の映像の H.264 エンコーダ。以前は FilmCraft の filmcraft-h264enc。テストでは MPEG-2 デコーダで MPEG-2 エンコーダの出力も検証する）、craft-media（MPEG-2 のプログラム / トランスポートストリーム、Matroska、MP4 の読み取り。規格に合った映像と音声を変換せずに取り出す）、craft-ui（パネルとワークスペースのドッキング）、craft-captions（ASS / SSA、SCC、MCC、EBU STL、TTML / DFXP、SAMI の字幕ファイル）、craft-mcp-server（`disccraft-cli mcp` の MCP サーバー）、craft-sniff（読み込んだファイルの種類を先頭のバイトから見分ける）、craft-settings（設定ファイルの `config.toml`）、craft-dialogs（フレームを止めないネイティブのファイルダイアログ）、craft-update（ヘルプ ▸ 更新を確認… での新しいリリースの確認・ダウンロード・インストール）、craft-fonts-build（ビルドで craft-fonts の書体を埋め込む。craft-ui の書体の組が Encore の書体の下に置く）を使い、テストで craft-psd（テスト用の PSD の書き出し）と craft-media-testkit（ffmpeg と比べるテストで ffmpeg と ffprobe を探す）も使う。リリースのビルドは、画面の書体に無い文字（アラビア文字、タイ文字、ヘブライ文字、記号など）のために craft-fonts（https://github.com/storytold/craft-fonts、OFL-1.1）の書体を埋め込む。書体とそのライセンスは NOTICE に載せる。

## 商標

「DVD」「Blu-ray Disc」は名称の説明的な使用のみで、ロゴは使わない。Adobe Encore は Adobe の商標で、DiscCraft は Adobe と関係がない。

## ライセンス

MIT OR Apache-2.0 のデュアルライセンス。詳細は LICENSE-MIT と LICENSE-APACHE を読む。明示的な合意がない限り、このリポジトリへの貢献も同じデュアルライセンスで提供される。
