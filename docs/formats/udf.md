# UDF 1.02 + ISO 9660 ブリッジのバイト配置（M5 の実装用）

DiscCraft が書く DVD-Video の ISO イメージの記述子レイアウト。公開規格（ECMA-119 第 4 版、ECMA-167 第 3 版、OSTA UDF 1.02）の記述から
まとめた。他の実装のソースは参照していない。オフセットは記述子の先頭からのバイト位置。UDF は全てリトルエンディアン。
外部オラクル: Windows のマウント（Mount-DiskImage）と libdvdread（ffmpeg の dvdvideo デマルチプレクサに ISO を直接渡す）。

## 共通の構造（UDF）

| 構造 | サイズ | 配置 |
|---|---|---|
| tag | 16 | id u16 @0, version u16 @2 (=2), checksum u8 @4 (bytes 0–3 と 5–15 の和 mod 256), reserved @5, serial u16 @6, CRC u16 @8 (tag の後ろ CRC 長ぶん), CRC 長 u16 @10, location u32 @12 |
| regid | 32 | flags u8 @0, identifier 23 bytes @1, suffix 8 @24 |
| charspec | 64 | type u8 @0 (=0), info 63 @1 ("OSTA Compressed Unicode") |
| timestamp | 12 | type+tz u16 @0 (0x1000), year u16 @2, month @4, day @5, hour @6, minute @7, second @8, cs @9, 100µs @10, µs @11 |
| extent_ad | 8 | length u32 @0 (bytes), location u32 @4 |
| short_ad | 8 | length u32 @0 (上位 2 bit = 種別 0), position u32 @4 (パーティション内 LBN) |
| long_ad | 16 | length u32 @0, LBN u32 @4, partition ref u16 @8, impl use 6 @10 |

## ボリューム記述子（絶対セクタ）

**PVD (tag 1)**, 512 bytes, CRC 長 496: VDS number u32 @16, primary VD number u32 @20, volume identifier d-string 32 @24,
volume sequence number u16 @56 (=1), max VSN u16 @58 (=1), interchange level u16 @60 (=2), max u16 @62 (=2), character set list u32 @64 (=1),
max u32 @68 (=1), volume set identifier d-string 128 @72, descriptor charset @200, explanatory charset @264, volume abstract extent_ad @328,
copyright extent_ad @336, application identifier regid @344, recording time @376, implementation identifier regid @388,
implementation use 64 @420, predecessor VDS location u32 @484, flags u16 @488.

**IUVD (tag 4)**, 512, CRC 長 496: VDS number u32 @16, implementation identifier regid @20 ("*UDF LV Info" + UDF suffix),
LVI charset @52, logical volume identifier d-string 128 @116, LV info 1/2/3 d-string 36 @244 / @280 / @316, implementation id regid @352,
implementation use 128 @384.

**PD (tag 5)**, 512, CRC 長 496: VDS number u32 @16, partition flags u16 @20 (=1), partition number u16 @22 (=0),
partition contents regid @24 ("+NSR02"), contents use 128 @56, access type u32 @184 (=1 read only), **starting location u32 @188**,
**length u32 @192**, implementation identifier regid @196, implementation use 128 @228.

**LVD (tag 6)**, 446 bytes, CRC 長 430: VDS number u32 @16, descriptor charset @20, logical volume identifier d-string 128 @84,
**logical block size u32 @212 (=2048)**, domain identifier regid @216 ("*OSTA UDF Compliant" + UDF suffix),
logical volume contents use 16 @248 = long_ad of the FSD (length 2048, LBN, partition 0), map table length u32 @264 (=6),
number of partition maps u32 @268 (=1), implementation identifier regid @272, implementation use 128 @304,
integrity sequence extent extent_ad @432, partition map @440: type u8 (=1), length u8 (=6), volume sequence number u16 (=1), partition number u16 (=0).

**USD (tag 7)**, 24 bytes, CRC 長 8: VDS number u32 @16, number of allocation descriptors u32 @20 (=0).
**TD (tag 8)**, 512, CRC 長 496.
**AVDP (tag 2)**, 512, CRC 長 496: main VDS extent_ad @16, reserve VDS extent_ad @24.

**LVID (tag 9)**, 134 bytes, CRC 長 118: recording time @16, integrity type u32 @28 (=1 close), next integrity extent extent_ad @32,
logical volume contents use 32 @40 (next unique id u64 @40), number of partitions u32 @72 (=1), length of implementation use u32 @76 (=46),
free space table u32 @80, size table u32 @84 (= partition length), implementation use @88: regid 32, number of files u32 @120,
number of directories u32 @124, min UDF read u16 @128 (0x0102), min write u16 @130, max write u16 @132.

## パーティション内（tag location はパーティション内 LBN）

**FSD (tag 256)**, 512, CRC 長 496: recording time @16, interchange level u16 @28 (=3), max u16 @30 (=3), character set list u32 @32 (=1),
max u32 @36 (=1), file set number u32 @40, file set descriptor number u32 @44, LVI charset @48, logical volume identifier d-string 128 @112,
file set charset @240, file set identifier d-string 32 @304, copyright file id d-string 32 @336, abstract file id d-string 32 @368,
root directory ICB long_ad @400, domain identifier regid @416, next extent long_ad @448.

**FE (tag 261)**, 176 + L_EA + L_AD bytes, CRC 長 = 全長 − 16: ICB tag @16 (prior recorded direct entries u32 @16, strategy type u16 @20 (=4),
strategy parameter u16 @22, max entries u16 @24 (=1), reserved u8 @26, file type u8 @27 (4 = directory, 5 = file), parent ICB lb_addr 6 @28,
flags u16 @34 (=0 short_ad)), uid u32 @36, gid u32 @40, permissions u32 @44, file link count u16 @48, record format u8 @50,
record display attributes u8 @51, record length u32 @52, information length u64 @56, logical blocks recorded u64 @64, access time @72,
modification time @84, attribute time @96, checkpoint u32 @108 (=1), extended attribute ICB long_ad @112, implementation identifier regid @128,
unique id u64 @160, L_EA u32 @168, L_AD u32 @172, extended attributes @176, allocation descriptors @176 + L_EA.

**FID (tag 257)**: file version number u16 @16 (=1), file characteristics u8 @18 (0x02 directory, 0x08 parent), L_FI u8 @19, ICB long_ad @20,
L_IU u16 @36 (=0), implementation use @38, file identifier @38 + L_IU (CS0: 8 + characters), padding to a multiple of 4. CRC 長 = FID 長 − 16.

## ISO 9660（ECMA-119）

**PVD**（セクタ 16）: type @0 (=1), "CD001" @1, version @6 (=1), system identifier 32 @8, volume identifier 32 @40,
volume space size both-endian @80 (LE u32 @80, BE u32 @84), volume set size both-endian u16 @120 (LE @120, BE @122),
volume sequence number @124 (LE @124, BE @126), **logical block size @128 (LE @128, BE @130) = 2048**, path table size @132 (LE @132, BE @136),
L path table u32 LE @140, optional L @144, M path table u32 BE @148, optional M @152, root directory record 34 @156,
volume set id 128 @190, publisher 128 @318, data preparer 128 @446, application 128 @574, copyright file id 37 @702, abstract 37 @739,
bibliographic 37 @776, creation date 17 @813, modification 17 @830, expiration 17 @847, effective 17 @864, file structure version @881 (=1).
Terminator（セクタ 17）: type 255, "CD001", version 1.

**Directory record**: length @0, extended attribute length @1, extent LE @2 / BE @6, data length LE @10 / BE @14, recording date 7 @18,
flags @25 (0x02 directory), file unit size @26, interleave gap @27, volume sequence number LE u16 @28 / BE @30, name length @32, name @33,
padding byte when the name length is even. **Path table entry**: name length @0, extended attribute length @1, extent @2 (LE in L, BE in M),
parent directory number @6 (LE / BE), name @8, padding to even.
