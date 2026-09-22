# Helper script for Noctalia lyric plugins
## 概要
再生されてる音楽を取得しプラグインに歌詞データを渡します
再生音楽の取得には`playerctl`を使用しています
`LRCLIB`から歌詞を取得しています
## 要件
以下のNoctalia pluginsに対応しています
- [lyrics](https://noctalia.dev/plugins/community/lyrics)
- [spotify-lyrics](https://noctalia.dev/plugins/community/spotify-lyrics)
[Pear Desktop](https://github.com/pear-devs/pear-desktop)で使用することを前提に開発されています
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
  $ noctalia-lyrics daemon
  ```
- 歌詞取得
  LIBLRCから歌詞を取得します
  現在再生されている音楽の歌詞として取得されます
  自動で歌詞が取得されないときに使用してください
  ```
  $ noctalia-lyrics get <LIBLRC_ID>
  ```
## ライセンス
MIT
