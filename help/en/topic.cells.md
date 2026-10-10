---
title: Cell database
short: Datasheet values of cell models, with proof for each value. The app later uses them to check live values and BMS parameters.
related: topic.derating_rule
---
- **Bundled:** verified entries from the project. They cannot be changed, but they can be **adapted**: this creates your own copy with the same id; the original can be restored.
- **Own:** created, duplicated or imported. They are stored in a file in the app data folder.
- **Verified** means the value was looked up in the datasheet. If the datasheet does not state a value, it is missing and the proof says "not specified".
- **Incomplete:** one of the values all checks need is missing or unverified.
- Only entries accepted by the project's JSON schema are saved.
