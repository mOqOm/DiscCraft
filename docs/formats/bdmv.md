# BDMV（Blu-ray）フォーマットのメモ

DiscCraft が書き出す `BDMV` フォルダの構造。クリーンルームの方針（AGENTS.md ルール 3）に従い、公開資料の記述だけから書いている。
他の実装のソースは読んでいない。検証は外部オラクル（ffmpeg の `bluray:` プロトコル = libbluray、ffprobe の mpegts / sup）で行う。

参照した公開資料:

- BluRay wiki（https://github.com/lw/BluRay/wiki 、ドキュメントのみの wiki リポジトリ、コミット 9d07b292、2026-11-08 参照）:
  INDX、MOBJ、NavigationCommand、MPLS（PlayList、PlayItem、STNTable、StreamEntry、StreamAttributes、PlayListMark）、
  CLPI（ClipInfo、SequenceInfo、ProgramInfo、StreamCodingInfo、CPI）、M2TS のフィールド表。
  wiki 自身は出典として米国特許出願 US 2010/0008647 A1、欧州特許出願 EP 1821310 A2、BDA のホワイトペーパー
  「BD-ROM Audio Visual Application Format Specifications」、HDcookbook、libbluray を挙げている
- ISO/IEC 13818-1（MPEG-2 システム: TS パケット、PES、PAT / PMT、PCR、CRC32）

## フォルダ構成

```
BDMV/
  index.bdmv            インデックステーブル（ファーストプレイ、トップメニュー、タイトル → ムービーオブジェクト）
  MovieObject.bdmv      ムービーオブジェクト（HDMV ナビゲーションコマンド）
  PLAYLIST/00001.mpls   タイトルごとのプレイリスト（プレイアイテム 1 つ、チャプター = エントリーマーク）
  CLIPINF/00001.clpi    クリップ情報（ストリーム、STC、EP マップ）
  STREAM/00001.m2ts     BDAV MPEG-2 TS
  BACKUP/               index.bdmv、MovieObject.bdmv、PLAYLIST、CLIPINF の写し
```

すべてのファイルは型識別子 4 バイト（`INDX` / `MOBJ` / `MPLS` / `HDMV`）と版 `0200` で始まり、多バイトの値はビッグエンディアン。
各ブロックの Length はその後ろのバイト数。

## m2ts

- ソースパケット 192 バイト = TP_extra_header 4 バイト（copy_permission_indicator 2 ビット = 0、arrival_time_stamp 30 ビット、27 MHz）
  + TS パケット 188 バイト。ファイルは 32 パケット（6144 バイト = 3 セクタ）のアラインドユニットの倍数。
- PID: PAT 0x0000、PMT 0x0100、PCR 0x1001（アダプテーションフィールドだけのパケット）、映像 0x1011、音声 0x1100〜、
  PG 0x1200〜、IG 0x1400〜。
- PMT の program_info に登録記述子 `05 04 'HDMV'`。stream_type: MPEG-2 映像 0x02、LPCM 0x80、AC-3 0x81、PG 0x90、IG 0x91。
- BD の LPCM は PES ごとに 4 バイトのヘッダ（audio_data_payload_size 16、channel_assignment 4、sampling_frequency 4、
  bits_per_sample 2、start_flag 1、予約 5）。ステレオ / 48 kHz / 16 ビットは `03 C0 31 40`（960 バイト = 5 ms）。
  サンプルはビッグエンディアンで、チャンネル数は常に偶数（モノラルは 2 チャンネルに詰める）。
- 多重化: 各 PES に締め切り（DTS）を付け、締め切りの早い順に一定のパケット間隔（既定 48 Mbps）で送る。映像は DTS の 0.5 秒前、
  音声は 0.25 秒前より早くは送らない。PCR は 40 ms ごと、PAT / PMT は 100 ms ごと。PCR の値 = ATS の基になる時計。

## CLPI の EP マップ

映像の I ピクチャ（シーケンスヘッダ付き）ごとに PTS と SPN を記録する。多重化では I ピクチャの PES の直前に必ず PAT / PMT / PCR を
置き、SPN はその PAT を指す（クリップは SPN 0 の PAT から始まり、どのエントリポイントからでもデコードを始められる。
libbluray は SPN 0 にエントリがないと「no timestamp for SPN 0」と警告する）。
細かい表は PTS_EP_fine = (PTS >> 9) & 0x7FF（11 ビット）、SPN_EP_fine = SPN & 0x1FFFF（17 ビット）。
粗い表は PTS_EP_coarse = (PTS >> 19) & 0x3FFF（14 ビット）、SPN_EP_coarse = SPN（32 ビット）、ref_to_EP_fine_id。
粗い値（PTS >> 19 か SPN >> 17）が変わるたびに粗いエントリを足す。

## PG（字幕）

- 参照した公開資料: PGS（.sup）の構造を説明した公開の技術解説（セグメント PCS 0x16 / WDS 0x17 / PDS 0x14 / ODS 0x15 / END 0x80、
  ランレングス符号）。
- TS では 1 セグメント = 1 PES（stream_id 0xBD、stream_type 0x90、PID 0x1200〜）。
- 字幕 1 つにつき、表示開始で PCS（エポック開始）+ WDS + PDS + ODS + END、終了で PCS（オブジェクトなし）+ WDS + END。
- パレット: 0 透明、1 白（文字）、2 黒（縁取り）。SRT は DVD と同じレンダラーで 2 ビットのビットマップにする。
- 検証: ffprobe が hdmv_pgs_subtitle を認識し、ffmpeg のオーバーレイで正しい時刻に文字が出る（日本語、2 行）。

## ISO イメージ（UDF 2.50）

BD の ISO は UDF 2.50 だけで書く（ISO 9660 なし）。ECMA-167 第 3 版と OSTA UDF 2.50 の記述から。
- 記述子のタグ版は 3、ボリューム認識は `BEA01` / `NSR03` / `TEA01`、パーティションの内容は `+NSR03`。
- 論理ボリュームはパーティションマップを 2 つ持つ: タイプ 1（物理）とタイプ 2（`*UDF Metadata Partition`）。
- 物理パーティションの先頭 2 ブロックにメタデータファイル（ファイル種別 250）とミラー（251）のエントリ、ブロック 32 から
  メタデータの本体とミラーの写し、その後ろにファイルのデータ。
- メタデータパーティションの中身: ブロック 0 = FSD、1 = TD、続いてディレクトリ（エントリ + FID）、ファイルのエントリ。
  LVD の内容使用欄は FSD（ブロック 0、パーティション参照 1）を指す。
- ファイルとディレクトリのエントリは拡張ファイルエントリ（EFE、タグ 266、基本部 216 バイト）。libudfread はメタデータファイルが
  EFE でないと読まない（「unexpected tag 261」）。ファイルのデータは long_ad（パーティション参照 0）で指す。
- 親ディレクトリの FID は識別子の長さ 0。
- 検証: libbluray（ffmpeg の `bluray:disc.iso`）が ISO を直接開いて全体をデコード、Windows のマウントで UDF として認識され
  全ファイルのハッシュが一致。

## 時刻の単位

- PES の PTS / DTS、EP マップ: 90 kHz
- MPLS の IN / OUT、マーク、CLPI の presentation_start / end: 45 kHz（90 kHz の値 / 2）
- ATS、PCR: 27 MHz

## 検証のメモ

- ffmpeg の `bluray:` 入力は 180 秒未満のプレイリストを無視する（「0 usable playlists」）。テストのタイトルは 190 秒にしている。
- `-chapter N` は libbluray が正しい位置にシークするが（デバッグログの `Seek to`）、ffmpeg がストリーム情報の取得で先頭に
  戻すので、ffmpeg ではチャプターの開始フレームを確かめられない。
- コーデックの違うクリップをつないだプレイリスト（AC-3 → LPCM）は、ffmpeg が最初のデコーダのまま読むためエラーになる。
  プレーヤーはプレイアイテムごとに STN テーブルを読み直すので、ディスクとしては正しい。
