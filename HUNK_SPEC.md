diffにカーソルがあるとき、ENTERが押されたらそのHUNKのみをstageに追加する。

# 実装イメージ
fn apply_hunk(指定するindex) {
    diff.foreach(
        &mut |_delta, _| true,e,
          None,
        Some(&mut |delta, hunk| {
            if hunkのindex == 指定されたindex {
                repo.apply_hunk(hunk, git2::ApplyLocation::Index)?;
            }
            true
        }),
    )?;
}

# 実装のポイント
カーソルが何番目のhunkにあるかをチェックする
hunk のindexだけで、どのhunkを適応するか判定する

