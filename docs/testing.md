# テスト方針

> 節番号（§n）は [初期設計書](adr/0001-initial-design.md) と共通。

## 10. 品質とテスト

| 種類 | 内容 |
|---|---|
| 単体・プロパティテスト | `proptest`。ビットリーダー / ライター、RLE、IFO / MPLS の往復 |
| 自前デコーダとの往復 | `mpeg2venc` の出力を `filmcraft-mpeg2v` でデコードし、エンコーダの再構成画像とビット一致を確認（FilmCraft の `h264enc` と同じ手法）。PSNR も記録。`ac3enc` は `filmcraft-ac3` で同様に |
| 外部オラクル（比較用） | ffmpeg / ffprobe は比較用（AGENTS.md ルール 9）。書き出した VOB、m2ts をエラー検出付きでデコードし、DiscCraft の読み戻しや VM と結果を比べる。テスト素材は ffmpeg で `target/fixtures/` に生成し、コミットしない。リンクも同梱もしない。ffmpeg がないときは、素材があればそれでテストを進め、ffprobe などとの比較だけを飛ばす |
| 構造の検証 | 書き出した `VIDEO_TS` / `BDMV` を `ifo` / `bdmv` で読み戻し、`vm` でファーストプレイから全ボタン・全リンクをたどる。到達できない要素、無限ループ、未設定のリンクを検出 |
| ソフトウェアプレーヤー | VLC などで再生する（外部ツールとして）。自動化はスクリーンショット比較までを目標 |
| 実機テスト | 家庭用の DVD / BD プレーヤーでの互換性チェックリスト（`docs/hardware-compat.md`）。人手が必要 |
| ファズテスト | すべてのパーサ（IFO、MPLS、CLPI、PSD、SRT、`.dcproj`）に切り詰め、ビット反転、サイズ改変。`catch_unwind` の下で実行 |
| ゴールデン画像 | メニュー描画とハイライト画像 |

品質ゲートは `cargo xtask ci`（fmt、`clippy -D warnings`、テスト、レイヤー、アセット）。コミットの前に必ず通す。
