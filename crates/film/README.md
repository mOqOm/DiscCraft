# disccraft-film

FilmCraft 連携。FilmCraft のプロジェクト（`.fcproj`）からシーケンスとマーカーを読み、マーカーをタイムラインの
チャプターポイントにする（engine の `filmcraft.sequences` / `timeline.importFilmCraftMarkers`）。

- プロジェクトのファイルは FilmCraft の `filmcraft-format`（スキーマの版の確認と古い版からの移行）と
  `filmcraft-project`（モデル）で読む。どちらも MIT OR Apache-2.0、rev はワークスペースの `Cargo.toml` で固定。
- 読むのはシーケンスの名前、長さ、マーカー（時刻、名前、チャプターマーカーかどうか）だけ。クリップやエフェクトは使わない。
- 入力の上限: ファイル 512 MB、1 シーケンスのマーカー 10,000 個。
