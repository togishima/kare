# kare（刈れ）

Test suite health check from CI artifacts — built for PHPUnit, accepts any JUnit XML.

テストは書くより消すのが難しい。kare は CI の成果物（JUnit XML）と実行履歴から flaky・実行時間の悪化・遅いテストを検出し、テスト削除のトリアージ材料を機械的に提供する CLI です。

## 設計思想

- **ツールはアタリを付け、人間が判断する** — テストの自動削除は絶対にしません
- **プロジェクトの外に立つ** — 対象言語のランタイム・依存には一切触れません
- **履歴が資産** — 実行を跨いだ差分・トレンド分析が本体です

> 🚧 開発初期段階のため、まだ実用できません。

## License

MIT
