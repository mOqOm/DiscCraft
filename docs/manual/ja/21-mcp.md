# 21. AI エージェントから使う（MCP）

[目次](README.md) ・ 前の章: [20. コマンドラインとスクリプト](20-command-line.md#20-コマンドラインとスクリプト) ・ 次の章: [22. 上限と決まり](22-limits.md#22-上限と決まり)

> **この章でできること**
>
> - AI エージェントから DiscCraft を使う

`disccraft-cli mcp` は MCP サーバー（標準入出力の JSON-RPC）として動く。ツールは 3 つ。

| ツール | 内容 |
|---|---|
| `disccraft_commands` | コマンドの一覧（ID、表示名、メニュー、ショートカット、パラメータ） |
| `disccraft_execute` | コマンド 1 つとパラメータを実行 |
| `disccraft_inspect` | 今のプロジェクトの検査とチェック結果 |

サーバーは Craft アプリ共通の MCP の仕組み（craft-libs の craft-mcp-server）で動く。プロトコルの版は 2025-06-18、
2025-03-26、2024-11-05 に対応し、知らない版を求められたら 2025-06-18 で答える。`disccraft_commands` と
`disccraft_inspect` は読むだけのツール、`disccraft_execute` はファイルを書くこともあるツールとして知らせる（保存、ビルド、
ディスクへの書き込みもコマンドでできるため）。ツールの引数は、一覧のとおりでなければ（`command` がない、知らない引数がある、
`params` がオブジェクトでない）エラーになる。

MCP クライアントには、コマンドに `<リポジトリのパス>/target/release/disccraft-cli`、引数に `mcp` を登録する
（標準入出力でつなぐ）。
