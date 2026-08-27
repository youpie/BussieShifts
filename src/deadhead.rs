/*
Hoe gaat dit werken
POD zoeker hoeft in principe maar 1 keer gegenereerd te worden per dienstregeling. Het is statische data
Een mat rit verschilt wel per dag, wanwege dienstregelingen & valid on dagen. Maar in principe kan het gewoon een lijst zijn die gerserveerd wordt met een request
Wel moeten meerdere bestemmingen samengevoegd worden met behulp van een soort look-up table (ehvsnel, ehvtraag, ehvgar, ehvgas) zijn allemaal eindhoven garague
misschien in het volgende formaat:
[Eindhoven garage]
ehvsnel
ehvtraag
...

[Eindhoven Busstation]
ehvbst
...


*/
