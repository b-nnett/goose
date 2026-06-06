# Build context MUST be the server/ dir (set in docker-compose.yml) so we can
# install the local whoop-protocol package.

# Stage 1: builder — install Python deps into /install prefix
FROM python:3.11-slim AS builder
WORKDIR /build
COPY ingest/requirements.txt /build/requirements.txt
RUN pip install --no-cache-dir --prefix=/install -r /build/requirements.txt

# Stage 2: runtime — lean image with pre-built wheels from builder
FROM python:3.11-slim AS runtime
WORKDIR /app
# Copy installed packages from builder stage
COPY --from=builder /install /usr/local
# Install local whoop-protocol package
COPY packages/whoop-protocol /app/whoop-protocol
RUN pip install --no-cache-dir /app/whoop-protocol
# Copy schema and application
COPY db /app/db
COPY ingest/app /app/app
# Run as non-root user (Trivy DS-0002)
RUN useradd --no-create-home --shell /bin/false appuser
USER appuser
EXPOSE 8000
CMD ["uvicorn", "app.main:app", "--host", "0.0.0.0", "--port", "8000"]
