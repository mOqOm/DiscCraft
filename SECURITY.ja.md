[English](SECURITY.md) | 日本語

# セキュリティの方針

DiscCraft は、自分で作っていないファイルを読みます。映像、音声、字幕、画像、Photoshop と Craft のドキュメント、
DiscCraft のプロジェクト、DVD と Blu-ray のディスク・フォルダー・イメージです。ディスクのイメージを書き出し、
ディスクの書き込みドライブを動かし、コマンドライン（`disccraft-cli`）と MCP のサーバー（`disccraft-cli mcp`）から
操作できます。新しいリリースを GitHub に問い合わせ、ダウンロードしてインストールすることもできます
（**ヘルプ ▸ 更新を確認…**）。これらのどれについての報告も歓迎します。

## 対象の版

DiscCraft はアルファ版（0.1.0-alpha）です。対象は [GitHub のリリース](https://github.com/mOqOm/DiscCraft/releases) の
最新の版だけで、修正は次のリリースに入ります。

## 脆弱性の報告

GitHub から非公開で報告してください。リポジトリの **Security** タブで **Report a vulnerability** を選びます
（[非公開の脆弱性の報告](https://github.com/mOqOm/DiscCraft/security/advisories/new)）。公開の issue は立てず、動く攻撃の
コード、悪意のあるファイル、人のプロジェクトを公開の場に載せないでください。

報告に入れてほしいこと:

- DiscCraft の版（ヘルプ ▸ DiscCraft について）と、システム（Windows、macOS、Linux と、プロセッサー）
- 何が影響を受け、攻撃者に何ができるか
- 再現の手順
- DiscCraft がうまく扱えないファイルなら、その形式・大きさ・SHA-256 のハッシュと、作ったものか誰かの素材か。
  ファイルそのものは、頼まれたときに非公開の報告の中で渡してください
- **ヘルプ ▸ ログファイルを表示…** で出るログ（思わぬところで終わったときは、その記録も）。
  個人のパスや、関係のないプロジェクトの中身は消してください

DiscCraft は 1 人で作っています。報告にはできるだけ早く答えますが、決まった期限はありません。詳細を公開する前に、
直すための時間をください。

## 対象になるもの

- DiscCraft は、どんな入力でも落ちたり固まったりしないことを目指しています。DiscCraft がパニックする、固まる、
  メモリーやディスクを度を越えて使うファイル・ディスク・プロジェクトは、報告してほしい不具合です。人を狙って
  使えそうなときは非公開で報告してください
- 利用者が選んだフォルダーの外を読み書きする、DiscCraft が意図しないものを動かす、DiscCraft 自身のリリース以外の
  ものを更新でインストールする
- DiscCraft が使うライブラリ（craft-libs、FilmCraft、PhotoCraft、EffectCraft、SoundCraft など）の問題も、ここに
  報告して構いません。そのプロジェクトに伝えます

対象外:

- コピープロテクション（CSS、AACS）。DiscCraft は保護されたディスクを読み書きしません
- プログラムはまだ開発者の証明書で署名していないため、初回の起動で Windows の SmartScreen と macOS の Gatekeeper が
  確認を求めます。これは分かっていることです
- DiscCraft の使い方の質問と、機能の要望は [issue](https://github.com/mOqOm/DiscCraft/issues) へお願いします
