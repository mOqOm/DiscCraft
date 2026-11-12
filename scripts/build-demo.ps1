# DiscCraft を Windows で確かめるための一式を target\demo に作る。
#
#   powershell -ExecutionPolicy Bypass -File scripts\build-demo.ps1
#
# 1. リリースビルド（GUI の disccraft.exe と CLI の disccraft-cli.exe）を target\demo\bin に置く
# 2. ffmpeg でテスト素材を target\fixtures に作る（既にあれば使う）
# 3. DVD（メニュー付き、M7 の全部入り、16:9）と Blu-ray（4:3、16:9）のデモを、フォルダと ISO の両方で書き出す
# 確認のしかたは target\demo\確認方法.txt に書き出す。

param(
    # ビルドを省き、既にある target\demo\bin を使う
    [switch]$SkipBuild
)

$ErrorActionPreference = 'Stop'
$root = Split-Path -Parent $PSScriptRoot
Set-Location $root

if (-not (Get-Command cargo -ErrorAction SilentlyContinue)) {
    $env:Path = "$env:USERPROFILE\.cargo\bin;$env:Path"
}
if (-not (Get-Command ffmpeg -ErrorAction SilentlyContinue)) {
    $found = Get-ChildItem "$env:LOCALAPPDATA\Microsoft\WinGet\Packages" -Recurse -Filter ffmpeg.exe -ErrorAction SilentlyContinue |
        Select-Object -First 1
    if ($null -eq $found) { throw 'ffmpeg が見つかりません。winget install Gyan.FFmpeg で入れてください。' }
    $env:Path = "$($found.DirectoryName);$env:Path"
}

$demo = Join-Path $root 'target\demo'
$fixtures = Join-Path $root 'target\fixtures'
$bin = Join-Path $demo 'bin'
New-Item -ItemType Directory -Force $demo, $fixtures, $bin | Out-Null

function Invoke-Tool {
    param([string]$Exe, [string[]]$ArgList)
    & $Exe @ArgList
    if ($LASTEXITCODE -ne 0) { throw "$Exe が失敗しました（終了コード $LASTEXITCODE）" }
}

function New-Fixture {
    param([string]$Name, [string[]]$ArgList)
    $path = Join-Path $fixtures $Name
    if (-not (Test-Path $path)) {
        Write-Host "素材を作成: $Name"
        Invoke-Tool ffmpeg (@('-v', 'error', '-y') + $ArgList + @($path))
    }
}

# --- 1. ビルド
if (-not $SkipBuild) {
    Write-Host 'リリースビルド中（初回は時間がかかります）'
    Invoke-Tool cargo @('build', '--release', '-p', 'disccraft', '-p', 'disccraft-cli')
    Copy-Item -Force (Join-Path $root 'target\release\disccraft.exe'), (Join-Path $root 'target\release\disccraft-cli.exe') $bin
}
$cli = Join-Path $bin 'disccraft-cli.exe'
if (-not (Test-Path $cli)) { throw "$cli がありません（-SkipBuild を外して実行してください）" }

# --- 2. 素材
New-Fixture 'ntsc.m2v' @('-f', 'lavfi', '-i', 'testsrc2=size=720x480:rate=30000/1001', '-t', '20', '-vf', 'setfield=tff',
    '-c:v', 'mpeg2video', '-pix_fmt', 'yuv420p', '-b:v', '6000k', '-maxrate', '8000k', '-bufsize', '1835k', '-g', '15', '-bf', '2',
    '-flags', '+cgop+ilme+ildct', '-sc_threshold', '1000000000', '-aspect', '4:3', '-f', 'mpeg2video')
New-Fixture 'ntsc.ac3' @('-f', 'lavfi', '-i', 'sine=frequency=1000:sample_rate=48000', '-t', '20', '-ac', '2', '-c:a', 'ac3', '-b:a', '192k')
New-Fixture 'h264_aac.mp4' @('-f', 'lavfi', '-i', 'testsrc2=size=1280x720:rate=24', '-f', 'lavfi', '-i', 'sine=frequency=660:sample_rate=44100',
    '-t', '6', '-c:v', 'libx264', '-pix_fmt', 'yuv420p', '-c:a', 'aac', '-shortest')
if (-not (Test-Path (Join-Path $fixtures 'slide1.png'))) {
    Write-Host '素材を作成: slide1-3.png'
    Invoke-Tool ffmpeg @('-v', 'error', '-y', '-i', (Join-Path $fixtures 'h264_aac.mp4'), '-vf', 'fps=1', '-frames:v', '3', (Join-Path $fixtures 'slide%d.png'))
}
New-Fixture 'bd_long.m2v' @('-f', 'lavfi', '-i', 'testsrc=size=720x480:rate=30000/1001', '-t', '190', '-vf', 'setdar=4/3',
    '-c:v', 'mpeg2video', '-b:v', '1500k', '-maxrate', '6M', '-bufsize', '1835k', '-g', '15', '-bf', '0', '-pix_fmt', 'yuv420p', '-f', 'mpeg2video')
# 16:9 のデモの素材: 1280x720 の中央に白い正方形（600x600）。縦横比が正しければ正方形に見える
New-Fixture 'wide.mp4' @('-f', 'lavfi', '-i', 'testsrc2=size=1280x720:rate=30000/1001', '-f', 'lavfi', '-i', 'sine=frequency=880:sample_rate=48000',
    '-t', '20', '-vf', 'drawbox=x=340:y=60:w=600:h=600:color=white:t=8', '-c:v', 'libx264', '-pix_fmt', 'yuv420p', '-c:a', 'aac', '-shortest')
# 本編も LPCM にして、本編とおまけをつないだプレイリストでも音声の形式がそろうようにする
New-Fixture 'bd_long.wav' @('-f', 'lavfi', '-i', 'sine=frequency=440:sample_rate=48000:duration=190', '-ac', '2', '-c:a', 'pcm_s16le')

# --- 3. デモディスク
function Build-Demo {
    param([string]$Name, [string]$Script)
    $dir = Join-Path $demo $Name
    if (Test-Path $dir) { Remove-Item -Recurse -Force $dir }
    New-Item -ItemType Directory -Force $dir | Out-Null
    $project = Join-Path $dir "$Name.dcproj"
    Write-Host "== $Name"
    Invoke-Tool $cli @('run', $project, $Script) | Out-Null
    Invoke-Tool $cli @('build', $project, '--out', (Join-Path $dir 'folder'))
    Invoke-Tool $cli @('build', $project, '--iso', (Join-Path $dir "$Name.iso"))
}

Build-Demo 'dvd-menu' (Join-Path $root 'docs\examples\menu-demo.jsonl')
Build-Demo 'dvd-m7' (Join-Path $root 'docs\examples\m7-demo.jsonl')
Build-Demo 'bluray' (Join-Path $root 'docs\examples\bd-demo.jsonl')
Build-Demo 'dvd-wide' (Join-Path $root 'docs\examples\dvd-wide-demo.jsonl')
Build-Demo 'bluray-wide' (Join-Path $root 'docs\examples\bd-wide-demo.jsonl')

# --- 4. 確認のしかた
$guide = @"
DiscCraft 確認用一式（$(Get-Date -Format 'yyyy-MM-dd HH:mm') 作成）

■ アプリ
  bin\disccraft.exe       GUI。ファイル → 開く で各フォルダの .dcproj を開ける
  bin\disccraft-cli.exe   CLI（disccraft-cli commands / inspect <.dcproj> / build <.dcproj> --out <dir> | --iso <file>）

■ DVD（VLC: メディア → ディスクを開く → DVD → 「参照」でフォルダ、または .iso をドラッグ）
  dvd-menu\folder\VIDEO_TS   3 ボタンのメニュー。上下キーと Enter で各チャプターへ。終わるとメニューに戻る
  dvd-menu\dvd-menu.iso      同じものの ISO（ダブルクリックで Windows にマウントもできる）
  dvd-m7\folder\VIDEO_TS     動画背景のモーションメニュー（ループ）、本編 2 本、スライドショー、
                             「続けて再生」で本編 A → B → メニュー
  dvd-m7\dvd-m7.iso

■ Blu-ray
  いちばん簡単: bluray\bluray.iso を VLC のウィンドウにドラッグ（メニューから始まる）
  フォルダで開く: メディア → ディスクを開く → Blu-ray →「参照」で bluray\folder を選ぶ
                 ※ その中の BDMV フォルダを選ぶと「入力を開くことができません」になる（VLC はディスクのルートを要求する）
  bluray\folder              トップメニュー（5 ボタン: 本編 / 第2章から / 第3章から / おまけ / 続けて再生）。
                             190 秒の本編（チャプター 3 つ、日本語字幕、LPCM 音声）は終わるとメニューに戻る
                             VLC の「字幕」メニューで字幕トラック（日本語）を選ぶと表示される
                             （VLC の設定「Blu-ray のメニューを使う」が有効なこと。既定で有効）
  bluray\bluray.iso          UDF 2.50 の ISO（Windows でもマウントできる）
  ※ HD（1080p）映像はまだ組み込み前。映像は DVD と同じ 720x480 の MPEG-2
  ※ ffmpeg で確かめる場合: ffprobe -i bluray:<フォルダか ISO>（180 秒未満のタイトルは ffmpeg 側で一覧に出ない）

■ 16:9（ワイド）
  dvd-wide\folder\VIDEO_TS   16:9 の DVD（スクイーズ）。メニュー（4:3）から「16:9 を再生」。
                             中央の白い枠が正方形に見えれば縦横比が正しい（横に伸びて見えたら 4:3 として扱われている）
                             字幕（日本語）、チャプター 3 つ、再生中のメニューボタンでメニューへ戻る
  dvd-wide\dvd-wide.iso
  bluray-wide\folder         16:9 の Blu-ray（720x480 の 16:9）。トップメニューから「16:9 を再生」。白い枠が正方形に見えるか
  bluray-wide\bluray-wide.iso

■ 見てほしいところ
  - メニューのボタンが上下キーで移動し、決定で再生が始まるか（DVD）
  - チャプターの次へ / 前へ（VLC の「再生」→「チャプター」）
  - 本編の後に指定した先（メニュー / 次のタイトル）へ進むか
  - BD の字幕が正しい時刻に出るか、ISO が開けるか
  - 16:9 のデモで白い枠が正方形に見えるか（DVD / Blu-ray）
"@
Set-Content -Path (Join-Path $demo '確認方法.txt') -Value $guide -Encoding UTF8
Write-Host ''
Write-Host "できました: $demo"
Write-Host "確認のしかた: $(Join-Path $demo '確認方法.txt')"
