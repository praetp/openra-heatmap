# OpenRA-heatmap

This program will take in an OpenRA replay and produce an image of the game's map with markers indicating game activity.

It the end, the goal is that this programm will take in multiple game replays of the same map and produce a heatmap that shows which parts of the map are 'hot'.

# How to run
- `cargo run <your replay file>` (the first time you do run this, it will build the program).
Note that you need the Rust build tools.

# But it does not work
The map needs to have a corresponding screenshot present on https://resource.openra.net/maps/.
Without a screenshot, it does not work. If your favorite map does not have a screenshot yet, maybe you can upload one ?

# Example output
![example output](example.png)
