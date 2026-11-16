# Черновик: оптимизация CI

Оригинальный запрос (verbatim):

оптимизация ci

1) использовать бинарник git-cliff вместо сборки, мб даже с проверкой подписи (<file>.sha512 )
 https://git-cliff.org/docs/installation/binary-releases/

`wget "https://github.com/orhun/git-cliff/releases/download/v${version}/git-cliff-${version}-x86_64-unknown-linux-gnu.tar.gz"`
или
`wget "https://github.com/orhun/git-cliff/releases/download/v${version}/git-cliff-${version}-x86_64-unknown-linux-musl.tar.gz"`

я не знаю что нам больше подойдет, musl или gnu. наверное gnu

2) использовать кеш https://github.com/Swatinem/rust-cache для сборки

пример (из https://github.com/zeroclaw-labs/zeroclaw/blob/master/.github/workflows/ci.yml)
```
      - uses: Swatinem/rust-cache@6323deb102c322ba6fcbdcafc7e3dddab59af2b6 # v2.9.2
        with:
          key: dev-fixture-v1
          cache-on-failure: true
          save-if: ${{ github.ref == 'refs/heads/master' }}
```
