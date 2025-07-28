```bash
curl -fsSL https://get.pnpm.io/install.sh | sh -
pnpm env use --global lts

# from this directory
pnpm i

pnpm run dev

# build release version
pnpm run build
node ./build/index.js
```