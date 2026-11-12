# AGENTS.md

DiscCraft で作業するエージェントと人のための、破ってはいけないルール。
[FilmCraft](https://github.com/storytold/filmcraft) の `AGENTS.md`（MIT OR Apache-2.0）を手本にしている。
設計は [docs/architecture.md](docs/architecture.md)、テスト方針は [docs/testing.md](docs/testing.md)、
進め方は [ROADMAP.md](ROADMAP.md) を参照。

## コマンド

| コマンド | 内容 |
|---|---|
| `cargo xtask ci` | 品質ゲート。fmt の検査 → `clippy -D warnings` → テスト → layers → assets。**コミットの前に必ず通す** |
| `cargo xtask fmt` | `cargo fmt --all` で整形する |
| `cargo xtask clippy` | `cargo clippy --workspace --all-targets -- -D warnings` |
| `cargo xtask test` | `cargo test --workspace` |
| `cargo xtask layers` | crate の層（docs/architecture.md §5.1）、GUI 依存の禁止、lint 設定、crate ルートの `#![deny]` を検査する |
| `cargo xtask assets` | `assets/` の各ファイルに `.attribution` があり、ライセンスが許可リストにあり、`ATTRIBUTION.md` に載っているかを検査する |

新しい crate を足すときは `xtask/src/layers.rs` の層の表にも追加する。

## ルール

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
9. **ffmpeg は比較用。** ffmpeg / ffprobe は、DiscCraft の出力と比べるための外部オラクルとしてだけ使う（書き出した VOB / m2ts を
   エラー検出付きでデコードできるか、ナビゲーションを読めるか、テスト素材の生成）。製品（GUI、CLI、ビルド、変換）からは呼ばない・
   リンクしない・同梱しない。ffmpeg がなくても DiscCraft はすべての機能が動き、テストは比べる部分だけを飛ばす
   （素材が `target/fixtures/` にあればそれを使う）。ソースは読まない（ルール 3）。VLC など他のプレーヤーも同じ扱い。
