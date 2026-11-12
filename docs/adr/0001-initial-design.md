# DiscCraft 設計書

> **版:** 0.1（ドラフト） · **作成日:** 2026-11-07 · **手本:** [storytold/filmcraft](https://github.com/storytold/filmcraft) v0.17.0（commit `da535c1`、2026-11-06） · **目標:** Adobe Encore CS6 相当の DVD / Blu-ray オーサリング

## 0. この文書について

DiscCraft の最初の設計書。FilmCraft（Pure Rust のオープンソース NLE。Premiere Pro とのパリティを目指している）の
リポジトリ構成・設計原則・開発ルールを手本にして、Encore 相当のディスクオーサリングソフトを作る方針をまとめる。

M0（リポジトリの骨組み）に着手するとき、この文書を次のファイルに分ける。

| 分割先 | 内容 | FilmCraft の対応ファイル |
|---|---|---|
| `README.md` | 概要、機能、使い方 | `README.md` |
| `AGENTS.md`（`CLAUDE.md` は `@AGENTS.md` のみ） | 破ってはいけないルール（§11） | `AGENTS.md`, `CLAUDE.md` |
| `docs/architecture.md` | レイヤー、データモデル、コマンド、パイプライン（§5〜§9） | `docs/architecture.md` |
| `ROADMAP.md`, `docs/target-app-parity.md` | Encore との機能対応表と進捗（§3, §12） | 同名ファイル |
| `docs/testing.md` | テスト方針（§10） | `docs/testing.md` |

## 1. 概要

**DiscCraft** は、映像・音声・静止画から DVD-Video / Blu-ray（BDMV）を作るオーサリングソフト。
メニュー、チャプター、複数の音声・字幕、ナビゲーションを組み立て、規格に合わせてトランスコードし、
フォルダ・ISO イメージ・ディスクに書き出す。

Adobe Encore は CS6（2012 年）で開発が終わり、Creative Cloud には含まれていない。代わりになる定番ツールがない分野で、
FilmCraft で編集した作品をディスクにする出口にもなる。

### ゴール

- Encore を使っていた人が迷わない画面構成と操作（プロジェクト、タイムライン、メニュー、フローチャート、ビルド）
- 家庭用プレーヤーで再生できる規格準拠の DVD-Video。次に Blu-ray（HDMV）
- Pure Rust、クリーンルーム実装、Windows / macOS / Linux
- すべての操作がコマンド。GUI、CLI、JSON 制御チャネル、MCP サーバーから同じ操作ができる（FilmCraft と同じ）
- クラッシュしない。作業を失わない

### 非ゴール（当面やらない）

- コピーガード（CSS、AACS など）。ライセンスが必要なため。リージョンコードのフラグ設定だけ対応する
- BD-J（Java）メニュー。Blu-ray は HDMV のみ
- DTS、Dolby TrueHD、DTS-HD のエンコード
- Encore の Flash（SWF）出力。代わりの Web 出力は検討のみ
- 動画編集。編集は FilmCraft に任せて連携する

## 2. FilmCraft から引き継ぐ設計原則

| FilmCraft の原則 | DiscCraft での適用 |
|---|---|
| **エンジン優先。** ウィンドウなしで全機能に届く | すべての変更は `Session::execute(id, params)` を通る。GUI はクライアントの一つ |
| **すべてがコマンド。** id、ラベル、メニュー位置、ショートカット、パラメータ、`enabled()`、`run()` | 同じ仕組み。名前空間は `asset.*`, `timeline.*`, `menu.*`, `button.*`, `nav.*`, `build.*` など（§7） |
| **正確な時間。** 整数 Tick（254,016,000,000 / 秒） | `filmcraft-time` をそのまま使う。90 kHz（PTS）、45 kHz（BD）、27 MHz（SCR）も整数で割り切れる（§9） |
| **コピーオンライトのスナップショット。** Undo はスナップショットの積み上げ | 同じ。ビルドジョブはスナップショットを持って裏で走り、編集を止めない |
| **厳格なレイヤー構造。** `cargo xtask layers` で強制 | 同じ。L0 のコーデック / フォーマット crate は単独で使える |
| **クラッシュしない。** `unwrap` / `expect` / `panic!` 禁止、入力はすべて敵対的 | 同じ |
| **クリーンルーム、Adobe 素材を使わない** | 同じ。Encore のテンプレート、ボタン、背景、アイコンは一切使わない |
| **外部オラクルでテスト。** ffmpeg / ffprobe はテストにだけ使い、リンクも同梱もしない | 同じ。ナビゲーションは自前の VM シミュレータでも検証する |
| **エージェントが操作できる。** MCP、`ui.screenshot`、ウィジェットの自動化 ID | 同じ |

## 3. 機能スコープ（Encore との対応表）

| 領域 | Encore の機能 | DiscCraft の方針 | 段階 |
|---|---|---|---|
| プロジェクト | プロジェクトパネル（アセット、タイムライン、メニュー、プレイリストの一覧） | 同等。アセットはパス参照。オフライン表示と再リンク | M1 |
| | プロジェクト設定（DVD / Blu-ray、NTSC / PAL） | 同等 | M1 |
| タイムライン | 映像 1 本、音声トラック、字幕トラック | 同等（上限は DVD / BD の規格どおり） | M1, M7 |
| | チャプターポイント（名前、ポスターフレーム） | 同等。FilmCraft のマーカーから自動作成も | M1, M9 |
| | 終了時アクション、メニューリモート、オーバーライド | 同等 | M3 |
| メニュー | 静止メニュー、ボタン（テキスト、サムネイル、ハイライト） | 同等 | M3 |
| | モーションメニュー（背景動画と音声、ループポイント、長さ） | 同等 | M7 |
| | ボタンのルーティング（自動 / 手動）、既定ボタン、タイムアウト | 同等 | M3 |
| | ハイライトのカラーセット（通常 / 選択 / 決定） | 同等 | M3 |
| | Photoshop でのメニュー編集 | PSD 読み込み。レイヤー名の規約でボタンとハイライトを認識 | M9 |
| | ライブラリ（テンプレート、ボタン、背景） | オリジナル素材で一から作る | M9 |
| ナビゲーション | ファーストプレイ、リンク、フローチャート | 同等 | M3, M6 |
| | プレイリスト、チャプタープレイリスト | 同等 | M7 |
| | スライドショー（静止画、トランジション、音声） | 同等。映像にレンダリングして書き出す | M7 |
| トランスコード | 自動トランスコード、品質プリセット | 同等。MPEG-2 と AC-3 のエンコーダを自作 | M4 |
| | ディスク容量に合わせたビットレート | 同等 | M4 |
| チェック | プロジェクトのチェック（リンク未設定、到達不能、上限超過など） | 同等 | M5 |
| ビルド | フォルダ（VIDEO_TS / BDMV） | 同等 | M2, M8 |
| | イメージ（ISO） | 同等 | M5 |
| | ディスクへの書き込み | 同等（OS の書き込み API） | M9 |
| | DVD マスター（DDP） | 対象外の候補 | — |
| | 容量メーター | 同等 | M5 |
| プレビュー | リモコン付きプレビュー | VM シミュレータの上で動くプレビュー | M6 |
| 連携 | Premiere / After Effects との Dynamic Link | FilmCraft の書き出しファイルとマーカーを取り込む | M9 |
| 出力 | Flash 出力 | 対象外 | — |
| 保護 | リージョンコード、コピー保護 | リージョンコードのみ | M5 |

## 4. 出力フォーマットの主な制約

一般に公開されている資料から集めた値。実装するときにもう一度確かめ、出典を各 crate の README に書く（FilmCraft の方式）。

### 4.1 DVD-Video

| 項目 | 値 |
|---|---|
| 映像 | MPEG-2（MP@ML）、MPEG-1。NTSC 720×480 29.97 fps、PAL 720×576 25 fps（704 幅、352 幅も可）。表示アスペクト 4:3 / 16:9 |
| 映像ビットレート | 最大 9.8 Mbps。多重化全体で最大 10.08 Mbps |
| GOP | NTSC 18 フレーム以下、PAL 15 フレーム以下 |
| 23.976p 素材 | プログレッシブで符号化し、3:2 プルダウンのフラグ（`repeat_first_field`）で 29.97 として再生させる |
| 音声 | LPCM（48 / 96 kHz）、AC-3、MPEG-1 Layer II（主に PAL 地域）、DTS（任意）。最大 8 ストリーム |
| 字幕（サブピクチャ） | 2 ビット 4 色（16 色パレットから選ぶ）、ランレングス符号化。最大 32 ストリーム |
| 構造 | 最大 99 タイトル、タイトルあたり最大 99 チャプター、最大 9 アングル、メニューあたり最大 36 ボタン |
| ファイル | `VIDEO_TS/` に IFO / BUP / VOB。VOB は 1 GB ごとに分割。2048 バイトセクター |
| VOBU | 0.4〜1.0 秒ごとに NAV パック（PCI / DSI） |
| ファイルシステム | UDF 1.02 + ISO 9660 ブリッジ |
| 容量 | DVD-5 約 4.7 GB（4.7×10⁹ バイト）、DVD-9 約 8.5 GB（2 層。レイヤーブレークの位置に制約あり） |

### 4.2 Blu-ray（BDMV / HDMV）

| 項目 | 値 |
|---|---|
| 映像 | H.264（High Profile, Level 4.1 まで）、MPEG-2、VC-1。1920×1080（23.976p / 24p / 59.94i / 50i）、1280×720（59.94p / 50p / 23.976p / 24p）など。SD 解像度も可 |
| ビットレート | 映像最大 40 Mbps、TS 全体で最大 48 Mbps |
| 音声 | LPCM、AC-3（プレーヤーの必須対応）。DTS / Dolby TrueHD / DTS-HD は対象外 |
| 字幕 | PG（Presentation Graphics）ストリーム |
| メニュー | IG（Interactive Graphics）ストリーム（常時表示 / ポップアップ）と HDMV ナビゲーションコマンド |
| 構造 | `BDMV/` に `index.bdmv`, `MovieObject.bdmv`, `PLAYLIST/*.mpls`, `CLIPINF/*.clpi`, `STREAM/*.m2ts`、`BACKUP/` に複製 |
| ストリーム | BDAV MPEG-2 TS（192 バイトのソースパケット） |
| ファイルシステム | UDF 2.50 |
| 容量 | BD-25 約 25 GB、BD-50 約 50 GB |

## 5. アーキテクチャ

FilmCraft と同じく、Cargo ワークスペースに小さな crate を並べ、レイヤーを `cargo xtask layers` で強制する。
crate 名は `disccraft-<dir>`（`crates/project` は `disccraft-project`）。

### 5.1 レイヤー

```text
 L6  apps/disccraft · apps/disccraft-cli
 L5  ui-egui · automation · platform
 L4  engine
 L3  build
 L2  menu · nav · transcode · check · dvd · bd                 （+ filmcraft-codecs）
 L1  project · vm                                              （+ filmcraft-frame · media · text · audio-dsp）
 L0  新規:      mpeg2venc · ac3enc · psmux · spu · ifo · m2tsmux · bdmv · pgs · udf
     再利用:    filmcraft-time · geom · color · bitstream · mpeg2v · mpegts · ac3 · h264 · h264enc
```

FilmCraft の crate は、FilmCraft 側のレイヤーと同じ位置に置いて扱う。

### 5.2 crate 一覧

| crate | 層 | 役割 |
|---|---|---|
| `mpeg2venc` | L0 | MPEG-2 ビデオエンコーダ（MP@ML）。DVD の制約（GOP 長、VBV、最大ビットレート）、インターレース / プログレッシブ、3:2 プルダウンのフラグ、CBR / VBR / 2 パス |
| `ac3enc` | L0 | AC-3 エンコーダ（2.0 / 5.1、192〜448 kbps） |
| `psmux` | L0 | MPEG-2 プログラムストリーム（VOB）の多重化。パック、PES、private stream 1（AC-3 / LPCM / サブピクチャ）、NAV パック（PCI / DSI）、VOBU アドレスの後埋め |
| `spu` | L0 | DVD サブピクチャの符号化と復号。2 ビット RLE、表示制御コマンド、ボタンのハイライト |
| `ifo` | L0 | DVD の IFO / BUP の読み書き。VMGI、VTSI、PGC、セル、PTT、VM コマンド |
| `m2tsmux` | L0 | BDAV TS の多重化。PAT / PMT、PCR、192 バイトのソースパケット、Aligned Unit |
| `bdmv` | L0 | `index.bdmv`, `MovieObject.bdmv`, MPLS, CLPI（EP マップ）の読み書き |
| `pgs` | L0 | BD の PG / IG セグメントの符号化。パレット、オブジェクト RLE、ボタン、HDMV コマンド |
| `udf` | L0 | UDF 1.02 / 2.50 と ISO 9660 のイメージ書き出し、読み戻し |
| `project` | L1 | ドキュメントモデル（§6） |
| `vm` | L1 | DVD 仮想マシンと HDMV コマンドのインタプリタ（GPRM / SPRM、リンク、ジャンプ）。プレビューとテストのオラクル |
| `menu` | L2 | メニューの描画。レイヤー、テキスト（`filmcraft-text`）、ボタン、セーフエリア → 背景フレームと 4 色のハイライト画像 |
| `nav` | L2 | ナビゲーションのコンパイラ。プロジェクトのリンク → 中間表現（NavIR）→ DVD VM コマンド / HDMV コマンド。「最後のメニューに戻る」などのためのレジスタ割り当て |
| `transcode` | L2 | 素材のデコード（`filmcraft-codecs`）、スケール、インターレース処理、色変換（BT.709 → BT.601）、フレームレート合わせ、エンコード。容量に合わせたビットレート配分 |
| `check` | L2 | プロジェクトの検証。リンク未設定、到達不能、上限超過、容量超過、規格違反 |
| `dvd` | L2 | プロジェクト → `VIDEO_TS`（VTS の構成、VOB、IFO / BUP） |
| `bd` | L2 | プロジェクト → `BDMV` |
| `build` | L3 | ビルドのパイプライン（トランスコード → オーサリング → イメージ → 書き込み）、ジョブ、進捗、キャンセル。トランスコード結果を内容ハッシュでキャッシュ |
| `engine` | L4 | `Session`、コマンドレジストリ、Undo、アセットプール、ジョブ |
| `ui-egui` | L5 | egui のフロントエンド（§8） |
| `automation` | L5 | MCP サーバー（stdio）。ヘッドレス、または起動中のアプリにブリッジ |
| `platform` | L5 | ディスク書き込みの OS API だけを扱う（Windows IMAPI2、macOS DiscRecording、Linux SG_IO）。`unsafe` を許す唯一の crate |
| `disccraft` | L6 | デスクトップアプリ（eframe / wgpu）、ファイルダイアログ、TCP 制御サーバー |
| `disccraft-cli` | L6 | ヘッドレス CLI：`exec`, `run`, `inspect`, `commands`, `build`, `probe`, `mcp` |

### 5.3 FilmCraft の crate の再利用

FilmCraft は MIT OR Apache-2.0。crates.io には公開されていないので、git 依存で `rev` を固定して使う。

```toml
[workspace.dependencies]
filmcraft-time = { git = "https://github.com/storytold/filmcraft", rev = "da535c18433dbe046553733b0ab08ce9bd736a51" }
filmcraft-codecs = { git = "https://github.com/storytold/filmcraft", rev = "da535c18433dbe046553733b0ab08ce9bd736a51" }
# ...
```

| FilmCraft の crate | DiscCraft での使い道 |
|---|---|
| `time`, `geom`, `color`, `bitstream` | 時間、座標、色変換、ビット読み書き |
| `frame`, `media`, `codecs` | 素材の読み込みとデコード（MP4 / MOV / MKV / MXF / MPEG-TS / PS、H.264 / HEVC / ProRes / DNx など） |
| `mpeg2v` | 自作 MPEG-2 エンコーダの検証、MPEG-2 素材をそのまま使えるかの判定 |
| `mpegts` | 書き出した VOB / m2ts の読み戻し検証 |
| `ac3` | 自作 AC-3 エンコーダの検証 |
| `h264enc`, `h264` | Blu-ray の映像エンコードと検証 |
| `text` | メニューの文字描画（OpenType シェーピング、日本語は craft-fonts） |
| `audio-dsp` | ラウドネス測定と正規化（EBU R128） |

- `h264enc` には現在 HRD / buffering period SEI とインターレース（MBAFF / PAFF）がない。Blu-ray に必要なので、まず FilmCraft へ上流の PR を検討し、難しければ fork する。
- egui は FilmCraft と同じ版（0.37）に合わせる。
- 再利用した crate は `NOTICE` と `README.md` に記載する。

### 5.4 ビルドのパイプライン

```text
アセット（MP4 / MOV / MXF / MKV / WAV / PNG / PSD / SRT …）
   │  filmcraft-codecs / media でデコード
   ▼
transcode ── 規格に合っていればパススルー（MPEG-2 / AC-3 / LPCM を検査し、再エンコードを省く）
   │         合っていなければ スケール → 色変換 → mpeg2venc / h264enc、音声は ac3enc / LPCM
   ▼  エレメンタリーストリーム（キャッシュ）
menu ── 背景（静止画 / 動画）+ ハイライト画像 → spu（DVD）/ pgs（BD）
   ▼
nav ── リンク → NavIR → DVD VM コマンド / HDMV コマンド
   ▼
dvd: psmux（VOB + NAV パック）→ ifo（IFO / BUP）    bd: m2tsmux → bdmv（CLPI / MPLS / index / MovieObject）
   ▼
udf（ISO）──► platform（書き込み）
   ▼
検証: ifo / bdmv で読み戻し、vm でファーストプレイから全リンクをたどる、mpegts / ffprobe で再読込
```

**DVD の構成方針（初期）。** メニューは VMGM ドメインに置き、タイムラインは 1 本ごとに 1 つの VTS にする（VTS は最大 99）。
音声・字幕の属性は VTS 単位で決まるので、この構成が一番単純。容量や互換性で問題が出たら見直す。

## 6. データモデル（`disccraft-project`）

```text
Project
├─ settings: DiscSettings
│    kind: Dvd | BluRay, standard: Ntsc | Pal, capacity: Dvd5 | Dvd9 | Bd25 | Bd50 | Custom(bytes)
│    regions, default_preset, first_play: Link, title_remote: Link
├─ root: Bin                              ビン（フォルダ）のツリー
├─ assets: map AssetId → Asset            パス、プローブ結果、オフライン、トランスコード設定
├─ timelines: map TimelineId → Timeline
├─ menus: map MenuId → Menu
├─ slideshows / playlists / chapter_playlists
└─ next_id

Timeline
├─ video: AssetRef（1 本）
├─ audio: Vec<AudioTrack>（DVD は最大 8）       言語コード付き
├─ subtitles: Vec<SubtitleTrack>（DVD は最大 32） 言語コード付き
├─ chapters: Vec<Chapter { time: Tick, name, poster: Tick }>   先頭は常に 0、昇順
├─ end_action: Link, menu_remote: Link, override_link: Option<Link>
└─ preset: Option<PresetRef>

Menu
├─ background: Still(AssetRef) | Motion { video, audio, loop_point: Tick, duration: Tick }
├─ layers: Vec<Layer>                    画像 / テキスト / 図形 / ボタングループ
├─ buttons: Vec<Button>
│    rect（ハイライト範囲）, layers, highlight: HighlightLayer, link: Link,
│    routing: Auto | Manual { up, down, left, right }, auto_activate: bool
├─ default_button, timeout: Option<(Tick, Link)>, end_action: Link
└─ color_set: ColorSet                   通常 / 選択 / 決定 の 4 色ずつ

Link = Timeline { id, chapter } | Menu { id, button } | Playlist { id } | ChapterPlaylist { id }
     | Slideshow { id } | LastMenu | Stop | NotSet
```

- すべて serde のデータ。`Project::check()` が不変条件（ID の一意性、チャプターの昇順、規格上限）を検査し、
  エンジンは編集のたびにこれを走らせる。
- プロジェクトファイルは `.dcproj`（JSON）。アトミック保存（一時ファイルに書いてからリネーム）、自動保存、
  クラッシュリカバリー用のジャーナル。スキーマのバージョンとマイグレーションを最初から入れる。
- メニューの座標はディスク解像度のピクセル。ピクセルアスペクト比は `DiscSettings` で決まり、画面表示では補正する。

## 7. コマンドシステム（`disccraft-engine`）

FilmCraft の `CommandSpec` と同じ形にする。

```rust
pub struct CommandSpec {
    pub id: &'static str,                 // "timeline.addChapter"
    pub label: &'static str,              // "チャプターを追加"
    pub menu: &'static [&'static str],    // ["タイムライン"]。空ならメニューに出さない
    pub shortcut: Option<&'static str>,
    pub params: &'static str,             // エージェントに見せるパラメータの説明
    pub enabled: fn(&Session) -> Result<(), String>,  // Err は理由
    pub run: fn(&mut Session, &Value) -> Result<Value>,
    pub journal: bool,                    // 読み取り専用のクエリは false
}
```

| 名前空間 | 例 |
|---|---|
| `file.*` | `new`, `open`, `save`, `saveAs` |
| `asset.*` | `import`, `relink`, `setPreset`, `probe` |
| `timeline.*` | `create`, `addChapter`, `moveChapter`, `chaptersFromMarkers`, `addAudioTrack`, `addSubtitleTrack`, `setEndAction` |
| `menu.*` | `create`, `fromTemplate`, `addLayer`, `addButton`, `autoRoute`, `setDefaultButton`, `setTimeout`, `setColorSet` |
| `button.*` | `setLink`, `setRouting`, `setHighlight` |
| `nav.*` | `setFirstPlay`, `setTitleRemote`, `setMenuRemote`, `setOverride` |
| `slideshow.*`, `playlist.*` | `create`, `addSlide`, `addEntry` |
| `disc.*` | `setKind`, `setCapacity`, `setRegions`, `usage`（クエリ） |
| `project.*` | `check`（クエリ）, `inspect`（クエリ） |
| `build.*` | `start`, `cancel` |
| `preview.*` | `open`, `key`（`up` `down` `left` `right` `enter` `menu` `title` `next` `prev`）, `state` |
| `jobs.*` | `list`, `cancel` |
| `edit.*` | `undo`, `redo` |

制御チャネル（TCP 上の JSON Lines）と MCP では次のように使う。

```jsonc
{"method": "engine.execute", "params": {"command": "asset.import", "params": {"path": "D:/media/movie.mp4"}}}
{"method": "engine.execute", "params": {"command": "timeline.create", "params": {"asset": 1}}}
{"method": "engine.execute", "params": {"command": "timeline.addChapter",
  "params": {"timeline": 2, "timecode": "00:05:00;00", "name": "第2章"}}}
{"method": "engine.execute", "params": {"command": "menu.create", "params": {"background": 3}}}
{"method": "engine.execute", "params": {"command": "button.setLink",
  "params": {"menu": 4, "button": 0, "target": {"timeline": 2, "chapter": 1}}}}
{"method": "engine.execute", "params": {"command": "nav.setFirstPlay", "params": {"target": {"menu": 4}}}}
{"method": "engine.execute", "params": {"command": "project.check"}}
{"method": "engine.execute", "params": {"command": "build.start", "params": {"output": "iso", "path": "D:/out/movie.iso"}}}
```

## 8. UI（`disccraft-ui-egui`）

egui のドッキングパネル。配置は Encore の作業の流れに合わせるが、アイコンと素材はすべてオリジナル。

| パネル | 内容 |
|---|---|
| プロジェクト | アセット、タイムライン、メニュー、スライドショー、プレイリストのビン表示 |
| モニター | タイムラインの映像、チャプターへの移動 |
| タイムライン | 映像・音声・字幕のトラック、チャプターマーカー |
| メニューエディター | キャンバス、タイトル / アクションセーフ、ハイライトの表示切り替え（通常 / 選択 / 決定）、ルーティングの矢印表示 |
| フローチャート | ノード（メニュー、タイムライン、プレイリスト）とリンクのグラフ。ドラッグでリンクを作る |
| プロパティ | 選択中の要素の設定（リンク、終了時アクション、タイムアウトなど） |
| レイヤー | メニューのレイヤーとボタンの構成 |
| ライブラリ | オリジナルのテンプレート、ボタン、背景 |
| ツール / 文字 | 選択、テキスト、図形、文字設定 |
| ビルド | 出力形式、出力先、容量メーター、進捗 |
| チェック / イベント | `project.check` の結果、エラーとジョブのログ |

- ワークスペース：標準、メニュー、ナビゲーション、ビルド。
- プレビューウィンドウ：VM シミュレータの上で動き、画面上のリモコンとキーボード（矢印、Enter、メニュー、タイトル）で操作する。
- 表示言語は日本語と英語から始める。日本語フォントは FilmCraft と同じく craft-fonts を任意のビルド入力にする。

## 9. 時間と単位

- 時間はすべて `filmcraft_time::Tick`（254,016,000,000 / 秒）。編集の計算に `f64` の秒は使わない。
- ディスク上の単位には境界で 1 回だけ変換する。どれも Tick で割り切れる。

| 単位 | 用途 | 1 単位あたりの Tick |
|---|---|---|
| 90 kHz | PTS / DTS、DVD のハイライト時刻 | 2,822,400 |
| 45 kHz | BD の CLPI / MPLS の時刻 | 5,644,800 |
| 27 MHz | SCR / PCR | 9,408 |
| フレーム | IFO の再生時間（BCD + フレームレートのフラグ） | `FrameRate::frame_duration()` |

- メニューの描画はリニアライトで行い、最後にディスクの色空間（SD は BT.601、HD は BT.709）に変換する。

## 10. 品質とテスト

| 種類 | 内容 |
|---|---|
| 単体・プロパティテスト | `proptest`。ビットリーダー / ライター、RLE、IFO / MPLS の往復 |
| 自前デコーダとの往復 | `mpeg2venc` の出力を `filmcraft-mpeg2v` でデコードし、エンコーダの再構成画像とビット一致を確認（FilmCraft の `h264enc` と同じ手法）。PSNR も記録。`ac3enc` は `filmcraft-ac3` で同様に |
| 外部オラクル | ffprobe / ffmpeg で VOB、m2ts をエラー検出付きでデコードする。テスト素材は ffmpeg で `target/fixtures/` に生成し、コミットしない。リンクも同梱もしない |
| 構造の検証 | 書き出した `VIDEO_TS` / `BDMV` を `ifo` / `bdmv` で読み戻し、`vm` でファーストプレイから全ボタン・全リンクをたどる。到達できない要素、無限ループ、未設定のリンクを検出 |
| ソフトウェアプレーヤー | VLC などで再生する（外部ツールとして）。自動化はスクリーンショット比較までを目標 |
| 実機テスト | 家庭用の DVD / BD プレーヤーでの互換性チェックリスト（`docs/hardware-compat.md`）。人手が必要 |
| ファズテスト | すべてのパーサ（IFO、MPLS、CLPI、PSD、SRT、`.dcproj`）に切り詰め、ビット反転、サイズ改変。`catch_unwind` の下で実行 |
| ゴールデン画像 | メニュー描画とハイライト画像 |

品質ゲートは `cargo xtask ci`（fmt、`clippy -D warnings`、テスト、レイヤー、アセット）。コミットの前に必ず通す。

## 11. ルール（`AGENTS.md` に入れる内容）

FilmCraft の `AGENTS.md` を手本にする（MIT OR Apache-2.0 なので、引用するときは出典を書く）。

1. **クラッシュしない。** テスト以外で `unwrap()`, `expect()`, `panic!`, `unreachable!`, `todo!`, `unimplemented!` を使わない。
   各 crate のルートに `#![deny(clippy::unwrap_used, clippy::expect_used, clippy::panic, clippy::unimplemented, clippy::todo, clippy::unreachable)]`。
   ワークスペースは `unsafe_code = "forbid"`。例外は `platform` だけで、FFI モジュールに限り、すべての `unsafe` に `// SAFETY:` を書く。
   入力から来る数値はすべて敵対的に扱う（`get()`、`checked_*`、0 除算の回避、入力サイズで決まる確保に上限）。
   ジョブは `catch_unwind` の下で動かし、失敗はエラーとして報告する。クラッシュの修正には、修正前に panic したテストを付ける。
2. **Adobe の素材を使わない。** Encore のテンプレート、ボタン、背景、アイコン、フォント、サンプル、スクリーンショットは
   一切使わず、トレースも「似せた」描き直しもしない。素材はすべてオリジナルかオープンライセンスで、各ファイルの横に
   `.attribution` を置き、`ATTRIBUTION.md` に 1 行追加する。
3. **クリーンルーム。** GPL / LGPL のコード（dvdauthor、libdvdnav、libdvdread、libbluray、FFmpeg、mjpegtools など）を読まない、写さない。
   実装中のフォーマットについては、パーミッシブなライセンスのものも含めて他の実装のソースを読まない。仕様書と公開資料から実装し、
   使った資料と版を crate の README に書く。他のツールは外部オラクル（ブラックボックス）としてだけ使う。
4. **ライセンスの許可リスト。** 依存と素材は MIT、Apache-2.0、BSD-2/3-Clause、ISC、Zlib、OFL-1.1、CC0-1.0、CC-BY-4.0、CC-BY-SA-4.0、
   パブリックドメインなど（FilmCraft と同じ）。MPL-2.0 は改変せずに使う場合のみ。
5. **商標。** 「DVD」「Blu-ray Disc」のロゴは使わない（使用にはライセンスが必要）。名称の説明的な使用のみ。
6. **特許。** MPEG-2 ビデオと AC-3 の主な特許は満了しているとされるが、H.264 は国によって有効な特許が残っている可能性がある。
   バイナリを配布する前に確認する。
7. **レイヤー。** 下の層から上の層に依存しない。L5 より下で egui / eframe / winit / rfd を使わない。
8. **コミット。** タスク ID ごとに 1 コミット（例：`M2.3: VOBU の DSI を生成`）。緑の状態だけをコミットする。

## 12. ロードマップ

| | 名称 | 内容 | 完了条件 |
|---|---|---|---|
| **M0** | 骨組み | `git init`、ワークスペース、xtask（fmt / clippy / test / layers / assets）、`README.md`, `AGENTS.md`, `ATTRIBUTION.md`、FilmCraft crate の git 依存、CI | 空のワークスペースで `cargo xtask ci` が通る |
| **M1** | プロジェクトとエンジン | `project`, `engine`、コマンド、Undo、`.dcproj` の保存と読み込み、CLI（`exec`, `commands`, `inspect`） | CLI で アセット取り込み → タイムライン作成 → チャプター追加 → 保存 → 読み込み ができる |
| **M2** | 最小の DVD ビルド | 規格に合った MPEG-2 + LPCM / AC-3 素材をパススルーで多重化。`psmux`（NAV パック込み）、`ifo`（VMG + VTS 1 本、チャプター）、`VIDEO_TS` フォルダ出力、読み戻し | ffmpeg で作った準拠素材から作った `VIDEO_TS` が VLC で再生でき、チャプター移動もでき、ffprobe がエラーを出さない |
| **M3** | DVD のメニューとナビゲーション | `menu`（静止メニュー）、`spu`（ハイライト）、`nav`（VM コマンド生成）、`vm`（シミュレータ）、ファーストプレイ、終了時アクション、最後のメニューに戻る | 3 ボタンのメニューから各チャプターへ飛び、終わるとメニューに戻るディスクが、VM シミュレータと VLC で動く |
| **M4** | トランスコード | `mpeg2venc`（CBR / VBR / 2 パス、DVD の制約）、`ac3enc`、スケール・色変換・プルダウン、容量に合わせたビットレート配分、キャッシュ | 任意の MP4 / MOV から DVD-5 に収まるディスクが作れ、出力を `filmcraft-mpeg2v` と ffmpeg がエラーなくデコードする |
| **M5** | イメージ出力とチェック | `udf`（UDF 1.02 + ISO 9660）、ISO 出力、`project.check`、容量メーター、リージョンコード | ISO をマウントして再生できる。既知の問題をすべて検出するチェックのテストがある |
| **M6** | GUI | egui のパネル（§8）、プレビュー（VM + リモコン）、制御チャネル、MCP サーバー | GUI だけで M3 相当のディスクが作れる。エージェントが MCP で同じことをできる |
| **M7** | DVD の残り | モーションメニュー、スライドショー、プレイリスト、チャプタープレイリスト、複数の音声・字幕（SRT → サブピクチャ）、16:9 とレターボックス、DVD-9（レイヤーブレーク） | §3 の DVD 機能が揃う |
| **M8** | Blu-ray | `m2tsmux`, `bdmv`, `pgs`（PG 字幕、IG メニューの常時表示 / ポップアップ）、`h264enc` の BD 対応（HRD / SEI、レベル、スライス数）、HDMV コマンド生成、UDF 2.50 | BD-25 の `BDMV` フォルダ / ISO がソフトウェアプレーヤーで再生でき、メニューを操作できる |
| **M9** | 仕上げと連携 | ディスク書き込み（Windows IMAPI2 から）、PSD メニューの読み込み、FilmCraft 連携（マーカー → チャプター）、テンプレートライブラリ、実機互換性テスト | — |

クリティカルパスは M2 → M3。最初はパススルーで多重化とナビゲーションを固め、エンコーダ（M4）はその後にする。
こうすると、一番難しい「家庭用プレーヤーで再生できるか」を早く確かめられる。

## 13. リポジトリ構成

```text
DiscCraft/
├─ Cargo.toml                ワークスペース（edition 2024、rust-version は FilmCraft に合わせる、lints）
├─ README.md · AGENTS.md · CLAUDE.md · ROADMAP.md · ATTRIBUTION.md · NOTICE
├─ LICENSE-MIT · LICENSE-APACHE
├─ apps/
│  ├─ disccraft/             デスクトップアプリ
│  └─ disccraft-cli/         ヘッドレス CLI、MCP サーバー
├─ crates/
│  ├─ mpeg2venc/  ac3enc/  psmux/  spu/  ifo/  m2tsmux/  bdmv/  pgs/  udf/     L0
│  ├─ project/  vm/                                                          L1
│  ├─ menu/  nav/  transcode/  check/  dvd/  bd/                             L2
│  ├─ build/                                                                 L3
│  ├─ engine/                                                                L4
│  └─ ui-egui/  automation/  platform/                                       L5
├─ assets/                   オリジナルのアイコンとテンプレート（各ファイルに .attribution）
├─ docs/                     architecture.md, testing.md, control-protocol.md,
│                            target-app-parity.md, hardware-compat.md, adr/
└─ xtask/                    ci, layers, assets
```

## 14. 未決事項とリスク

### 決めること（推奨案つき）

| 項目 | 推奨 | 理由 |
|---|---|---|
| FilmCraft の crate の取り込み方 | git 依存で `rev` 固定 | 上流の修正を取り込みやすい。足りない機能は上流に PR、だめなら fork |
| ライセンス | MIT OR Apache-2.0 | FilmCraft と揃えると、コードやドキュメントの行き来が楽 |
| 最初の対象 | DVD を先に、Blu-ray は M8 | DVD のほうが構造が単純で、Encore の中心的な用途 |
| 優先する OS | Windows（今の開発環境）を最優先、CI で 3 OS | 書き込み API は OS ごとに別実装になる |
| 公開先 | 未定（GitHub 公開か非公開か） | — |
| UI の言語 | 日本語と英語 | — |

### リスク

| リスク | 対策 |
|---|---|
| DVD-Video と BD-ROM の正式な仕様書はライセンス制で、公開されていない | 公開されている解説資料と実機の挙動から実装する。出典を README に残す。実機テストを M2 から始める |
| 家庭用プレーヤーの互換性のばらつき | 保守的な構成（NAV パック、VOBU 長、ビットレートに余裕）を既定にする。互換性チェックリストを育てる |
| MPEG-2 エンコーダの画質が DVD の画質を決める | 2 パス VBR、適応量子化、シーンチェンジ検出を段階的に入れる。PSNR / SSIM を記録して退行を防ぐ |
| BD の IG メニューは複雑 | 常時表示メニューを先に作り、ポップアップは後にする |
| `h264enc` に BD に必要な機能（HRD、インターレース）がない | 1080p 23.976 / 24（プログレッシブ）から始める。HRD は上流 PR |

## 15. 参考資料

| 資料 | 用途 | 入手 |
|---|---|---|
| ITU-T H.262 \| ISO/IEC 13818-2 | MPEG-2 ビデオ | ITU-T 版は無料 |
| ITU-T H.222.0 \| ISO/IEC 13818-1 | プログラムストリーム、トランスポートストリーム | ITU-T 版は無料 |
| ITU-T H.264 | Blu-ray の映像 | 無料 |
| ATSC A/52 | AC-3 | 無料 |
| ECMA-119（ISO 9660）、ECMA-167、OSTA UDF 1.02 / 2.50 | ファイルシステム | 無料 |
| ITU-R BT.601 / BT.709 | 色空間 | 無料 |
| DVD-Video の仕様（DVD Format Book） | IFO、NAV パック、VM | ライセンス制。公開されている解説資料を使う |
| BD-ROM の仕様 | BDMV、IG / PG | ライセンス制。BDA の公開ホワイトペーパーと公開されている解説資料を使う |
| FilmCraft の `README.md`, `AGENTS.md`, `docs/architecture.md`, `ROADMAP.md` | 構成と運用の手本 | GitHub |
