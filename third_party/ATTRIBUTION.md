# Third-party data

## GeoNames cities15000

`GeoNames_cities15000.txt` is derived from GeoNames (`cities15000.zip`), © GeoNames
contributors, licensed **CC-BY 4.0** — https://www.geonames.org/export/cities.html.

Attribution is shown in the app's search palette footer and in the site footer.

Redistribution: the file is checked in because the dataset build is reproducible from it
and the deploy target is a static host with no build-time network guarantees. Only cities
with population >= 50 000 survive into the shipped index; the raw file is not served.

## IANA time zone database

Timezone rules come from the `chrono-tz` crate, which embeds the IANA tzdb
(BSD-style licence, https://www.iana.org/time-zones). No tzdb file is vendored here.
