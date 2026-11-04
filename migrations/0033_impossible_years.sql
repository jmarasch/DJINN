-- Some files carry a placeholder like 9999 or 0 as their year. Those mean
-- nothing, and one of them stretched the year lists by thousands of years.
-- Nothing else re-reads an unchanged file, so clear what's already stored.
UPDATE tracks SET year = NULL WHERE year < 1000 OR year > 2100;
UPDATE albums SET year = NULL WHERE year < 1000 OR year > 2100;
