# kare（刈れ）

**Prune your test suite.** Deletion triage for your tests, from CI artifacts and run history — built for PHPUnit, accepts any JUnit XML.

テストは書くより消すのが難しい。kare は「どのテストを刈るべきか」の判断材料を提供する CLI です。CI の成果物（JUnit XML）と実行を跨いだ履歴から flaky・実行時間の悪化・遅いテストを検出し、削除候補のトリアージ材料として提示します。

## 設計思想

- **ツールはアタリを付け、人間が判断する** — テストの自動削除は絶対にしません
- **プロジェクトの外に立つ** — 対象言語のランタイム・依存には一切触れません（composer を汚しません）
- **履歴が資産** — 実行を跨いだ差分・トレンド分析が本体です

## 既存ツールとの違い

flaky 検出や slow テスト検出そのものは既存のツールやサービスでも可能です。kare の目的は検出ではなく**テスト削除のトリアージ**であり、その判断材料を外部サービスに送らず手元の履歴 DB に蓄積することです。

| | 実行を跨いだ履歴分析 | ランタイム非依存 | ローカル完結 | 削除トリアージ |
|---|---|---|---|---|
| PHPUnit 拡張（speedtrap 等） | — | —（composer 依存） | ✅ | — |
| flaky 検出 SaaS（BuildPulse, Codecov 等） | ✅ | ✅ | —（結果を外部送信） | — |
| レポーティング基盤（Allure, ReportPortal） | 部分的 | ✅ | 要サーバー | — |
| **kare** | ✅ | ✅ | ✅ 単体バイナリ + SQLite | ✅ |

> 🚧 開発初期段階のため、まだ実用できません。

## License

MIT
