# 20. コマンドラインとスクリプト

[目次](README.md) ・ 前の章: [19. キーボードショートカット](19-shortcuts.md#19-キーボードショートカット) ・ 次の章: [21. AI エージェントから使う（MCP）](21-mcp.md#21-ai-エージェントから使うmcp)

> **この章でできること**
>
> - コマンドラインでプロジェクトを作り、ビルドする

GUI と同じ操作は `disccraft-cli` からでもできる（ビルド、プロジェクトの検査、スクリプトの実行）。

```
cargo run -p disccraft-cli -- commands [--json]
cargo run -p disccraft-cli -- exec <project.dcproj> <command> [params-json]
cargo run -p disccraft-cli -- run <project.dcproj> <script.jsonl>
cargo run -p disccraft-cli -- inspect <project.dcproj> [--json]
cargo run -p disccraft-cli -- build <project.dcproj> --out <dir>
cargo run -p disccraft-cli -- build <project.dcproj> --iso <file> [--cache <dir>] [--audio-language ja]
cargo run -p disccraft-cli -- build <project.dcproj> --ddp <dir>
cargo run -p disccraft-cli -- bd-chapters <D: | ディスクのフォルダー | .mpls> [--playlist 1 | --all] [--json | --ogm]
cargo run -p disccraft-cli -- disc-info [--recorder E:]
```

- `commands` はエンジンのコマンドの一覧（ID、表示名、メニューの場所、ショートカット、パラメータ）。`--json` で JSON
- `exec` はコマンド 1 つを実行して結果を JSON で出す。プロジェクトが未保存なら保存する
- `run` は JSON Lines のスクリプトを実行する。1 行が 1 コマンドで `{"command": "...", "params": {...}}`。
  `#` と `//` で始まる行は無視。スクリプトは 16 MB まで
- `inspect` はプロジェクトの要約（`--json` で `project.inspect` の JSON）
- `build` は `--out` でフォルダー、`--iso` でイメージ、`--ddp` で DVD の DDP イメージ（そのフォルダーに）を作る。
  `--cache` で変換した素材の置き場所を、
  `--audio-language` で言語の付いていない音声の言語（ISO 639-1）を指定する
- `bd-chapters` は Blu-ray のプレイリストの一覧（長さ、チャプターの数、クリップ）と、いちばん長いメインのプレイリストのチャプター（始まりの時刻と長さ）を出す。

  > **詳しく**
  >
  > - ドライブのディスク（`D:`）、ディスクのフォルダー、その `BDMV` か `PLAYLIST` のフォルダー、`.mpls` ファイル 1 つのどれでもよい
  > - `--playlist 2` でほかのプレイリスト、`--all` ですべてのプレイリストのチャプター、`--json` で全部を秒の JSON、`--ogm` で OGM のチャプターファイル（`CHAPTER01=00:00:00.000`）の形
  > - 同じ映像の Blu-ray と同じ位置にチャプターポイントを置くときに使える
  > - 読むのはプレイリストだけ（暗号化されたディスクもプレイリストは暗号化されていないが、その映像は使えない）
- `disc-info` は、レコーダーに入っているディスクについてドライブが答えることを出す: DVD+R と DVD+RW のメディア ID、空か、
  追記できるか、閉じているか、セッションの数、トラックごとの始まりと大きさ、予約されているか、空か、次に書く位置、最後に
  書いたセクター、2 層のディスクなら 1 層目（空の DVD+R DL に前に止まった書き込みがあるかも）。何も書かない。書き込みに失敗したディスクに何が残ったかを調べるときに使う（Windows。`recorders` と
  `burn` は [ビルドする](14-build.md#14-ビルドする) にある）
