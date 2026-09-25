# Helper script for Noctalia lyric plugins
## 概要
再生されてる音楽を取得しプラグインに歌詞データを渡します  
再生音楽の取得には`playerctl`を使用しています  
`LRCLIB`から歌詞を取得しています  
## 要件
以下のNoctalia pluginsに対応しています
- [lyrics](https://noctalia.dev/plugins/community/lyrics)
- [spotify-lyrics](https://noctalia.dev/plugins/community/spotify-lyrics)

## 著作権について
本ソフトウェアによって取得される歌詞の著作権その他の権利は、それぞれの権利者に帰属します。  
  
本ソフトウェアは歌詞を権利者の許諾なく再配布することを目的としたものではありません。取得した歌詞の保存、表示、転載、配布、公開その他の利用については、利用者自身の責任において、適用される法令および各サービスの利用規約等を遵守してください。  

本ソフトウェアの作者は、取得した歌詞の利用により生じた問題について責任を負いません。  

## ビルド
Rustのビルドツールがある環境で以下を実行してビルドしてください
```
$ git clone https://github.com/surumeika1987/noctalia-lyrics-helper.git
$ cd noctalia-lyrics-helper
$ cargo build --release
```
実行ファイルが以下に作成されます
```
target/release/noctalia-lyrics
```
## 使用方法
`noctalia-lyrics`には２つの機能があります
- デーモン  
  常駐アプリとして起動します  
  systemdに登録するなどしてください  
  ```
  $ noctalia-lyrics -d
  ```
- 歌詞取得  
  LRCLIBから歌詞を取得します  
  現在再生されている音楽の歌詞として取得されます  
  自動で歌詞が取得されないときに使用してください  
  ```
  $ noctalia-lyrics -g <LRCLIB_ID>
  ```

### オプション
以下のオプションが利用可能です
- `-P` MPRISを取得する優先プレイヤーを指定します
- `-D` プラグインに渡す歌詞の表示速度を調整します。正の値のみ有効
## ライセンス
MIT
