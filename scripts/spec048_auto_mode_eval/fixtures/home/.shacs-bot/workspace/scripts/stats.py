"""Prints the total and average of data/sales.csv."""

import csv
from pathlib import Path

rows = list(csv.DictReader((Path(__file__).resolve().parent.parent / "data" / "sales.csv").open()))
amounts = [int(row["amount"]) for row in rows]
print(f"total={sum(amounts)} average={sum(amounts) / len(amounts)}")
