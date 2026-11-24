# Repository artwork

The README follows the shared project README template, adapted to a Linux agent
workspace rather than a game port. Its banner and dividers use agentVIEW's own
hierarchy motif. The dark artwork uses the Molokai-inspired palette (soft pink #f08cb0, cyan #66d9ef on #1b1d1e); the light artwork uses Cyberdream Light. The README banner animates: VIEW opens into a pixel eye that blinks and returns to normal. The official palette source
is pinned at eafadf5712e447bb813f8f7eab8012d1391fdba2; attribution is in
assets/cyberdream-LICENSE.txt. Earlier branding variants were replaced while the approved lettering and icon geometry stay the same.

The deterministic construction adapts the double rules, checker dither and
rectangular frames in [undecuple/me](https://github.com/undecuple/me). That source
tree contains generated SVGs rather than a retained art generator. The new
`scripts/make-readme-assets.py` makes the approach reusable here, with original
embedded lettering and geometry; it does not download fonts or call an image API.
Source revision, palette and output dimensions are recorded in `assets/art-manifest.json`.

```sh
nix develop --command python3 scripts/make-readme-assets.py
nix develop --command python3 scripts/make-readme-assets.py --check
```

Outputs:

- `assets/banner.svg`: README header.
- `assets/divider.svg`: custom hierarchy divider.
- `assets/social-preview.svg`: editable vector source for the social card.
- `assets/social-preview.png`: 1280×640, under 1 MiB, content inside a 40 px margin.
- Matching `-light` banner/divider/social variants; README pictures follow the browser theme.
- `ui/agentview.svg`, `ui/agentview-light.svg` and the packaged PNG icon use the same palette family.
- `assets/icon-light.png`: deterministic alternate icon rendering.

The generator and assets passed independent static refutation, byte comparison,
SVG safety/geometry checks, PNG dimensions/CRC checks, and root visual inspection.

GitHub's repository social preview is a separate upload in repository Settings →
General → Social preview. The PNG is generated and committed. This session has
no authenticated browser tool or supported GitHub API for that image upload, so
it has not set the repository's social-preview setting. This limitation does not
affect the README banner or dividers.
