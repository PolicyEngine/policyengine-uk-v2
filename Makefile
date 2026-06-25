PYTHON := uv run python
DATA_DIR := data

.PHONY: help build targets data data-upload data-year report clean-data

help:
	@echo "Targets:"
	@echo "  build         Build the Rust binary + native extension into the Python package"
	@echo "  targets       Regenerate data/calibration_targets.json from raw source files"
	@echo "  data          Rebuild all EFRS years from scratch (no upload). Runs build + targets first."
	@echo "  data-upload   As 'data', but upload the rebuilt clean CSVs to GCS"
	@echo "  data-year     Rebuild a single year, e.g. make data-year YEAR=2026"
	@echo "  report        Regenerate site/public/calibration.json from the latest diagnostics"
	@echo "  clean-data    Remove the local clean/ build outputs"

build:
	./interfaces/python/build_package.sh

targets:
	$(PYTHON) $(DATA_DIR)/build_targets.py

# Full from-scratch rebuild: the binary the calibration baseline runs, the
# calibration targets, then every EFRS year (real survey years + forecast
# years). Raw FRS/WAS/LCFS/SPI inputs are pulled from GCS by efrs.py as needed.
data: build targets
	cd $(DATA_DIR) && $(PYTHON) efrs.py --no-upload
	$(PYTHON) $(DATA_DIR)/calibration_report.py

data-upload: build targets
	cd $(DATA_DIR) && $(PYTHON) efrs.py
	$(PYTHON) $(DATA_DIR)/calibration_report.py

data-year: build targets
	@test -n "$(YEAR)" || { echo "Usage: make data-year YEAR=2026"; exit 1; }
	cd $(DATA_DIR) && $(PYTHON) efrs.py --year $(YEAR) --no-upload
	$(PYTHON) $(DATA_DIR)/calibration_report.py

# Regenerate the calibration payload (site/public/calibration.json) from
# whatever diagnostics already exist in data/clean/calib_diag/ (no data rebuild).
# The docs site's Calibration section reads this file.
report:
	$(PYTHON) $(DATA_DIR)/calibration_report.py

clean-data:
	rm -rf $(DATA_DIR)/clean
