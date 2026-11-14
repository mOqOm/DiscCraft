# disccraft-ac3enc

DVD / Blu-ray の変換した音声（48 kHz・16 ビットの WAV、モノラルかステレオ）を AC-3（Dolby Digital）の生のストリームにする。

- 符号化: craft-libs の `craft-audio`（MIT OR Apache-2.0、revision 47107f660191de2f0b425569506f4daa4faadac9、feature
  `ac3-encode`）。ETSI TS 102 366 V1.4.1 から書かれたエンコーダを公開 API（`ac3::encoder::{Encoder, Config}`）からだけ
  使い、その実装は読まない（AGENTS.md ルール 3）。
- このクレートが受け持つこと: WAV（RIFF）の読み取り（ファイルより長いと書かれたデータは、ファイルの終わりまで）、
  ビットレートの選択（ステレオ 192 kbps、モノラル 96 kbps）、エンコーダの前置き（解析窓による 256 サンプル）の分を
  入力の前から落として、デコードした音が元の音とサンプル単位で揃うようにすること。
- 確かめ方: 単体テスト（ヘッダーの読み取り、ほかの種類の WAV を断る、1 秒のトーンが 768 バイトの 32 フレーム）と、
  ffmpeg を外部オラクルにしたテスト（1.000 秒のクリックが 1.000 秒に聞こえる）。
