#!/usr/bin/env python3
"""
Test script for natural language to SQL conversion
"""

import os
import json
import requests

# Simulate the database context
db_context = {
    "db_type": "Postgres",
    "database": "railway",
    "tables": [
        # We'll discover these from the actual connection
    ]
}

# Test natural language queries
test_queries = [
    "show me all tables",
    "get the first 10 rows from users table",
    "find all records created in the last 7 days",
    "count total number of users"
]

def build_prompt(nl_query: str, tables: list) -> str:
    """Build the prompt exactly as the Rust code does"""
    context = f"Database Type: {db_context['db_type']}\n"
    context += f"Database: {db_context['database']}\n"
    
    if tables:
        context += "\nAvailable Tables/Collections:\n"
        for table in tables:
            context += f"- {table}\n"
    
    prompt = f"""You are a database query assistant. Convert natural language requests into valid database queries.

{context}

Rules:
- For PostgreSQL: Generate valid SQL SELECT statements
- For MongoDB: Generate valid MongoDB query JSON
- Return ONLY the query, no explanations
- Use proper table/collection names from the schema
- Be concise and accurate

User request: {nl_query}"""
    
    return prompt

def test_with_gemini(prompt: str, api_key: str):
    """Test with Gemini API"""
    url = "https://generativelanguage.googleapis.com/v1beta/models/gemini-2.0-flash-exp:generateContent"
    
    headers = {
        "Content-Type": "application/json",
    }
    
    payload = {
        "contents": [{
            "parts": [{
                "text": prompt
            }]
        }],
        "generationConfig": {
            "temperature": 0.1,
            "maxOutputTokens": 200,
        }
    }
    
    response = requests.post(f"{url}?key={api_key}", headers=headers, json=payload)
    
    if response.status_code == 200:
        result = response.json()
        if 'candidates' in result and len(result['candidates']) > 0:
            text = result['candidates'][0]['content']['parts'][0]['text']
            # Clean up markdown code blocks
            text = text.strip()
            text = text.removeprefix("```sql").removeprefix("```json").removeprefix("```")
            text = text.removesuffix("```")
            return text.strip()
    return None

def get_postgres_tables(conn_string: str):
    """Get actual table names from PostgreSQL"""
    import psycopg2
    try:
        conn = psycopg2.connect(conn_string)
        cur = conn.cursor()
        cur.execute("""
            SELECT table_name 
            FROM information_schema.tables 
            WHERE table_schema = 'public' 
            ORDER BY table_name
        """)
        tables = [row[0] for row in cur.fetchall()]
        cur.close()
        conn.close()
        return tables
    except Exception as e:
        print(f"Error getting tables: {e}")
        return []

if __name__ == "__main__":
    # Check for Gemini API key
    api_key = os.environ.get("GEMINI_API_KEY")
    if not api_key:
        print("❌ GEMINI_API_KEY not set in environment")
        print("Please set it first: export GEMINI_API_KEY=your_key_here")
        exit(1)
    
    # Try to get real table names
    conn_string = "postgresql://postgres:BRekrvFMzlBZkuGrJogNBRVFnFQWLqZf@nozomi.proxy.rlwy.net:59255/railway"
    
    print("🔍 Connecting to database to get table list...")
    try:
        import psycopg2
        tables = get_postgres_tables(conn_string)
        if tables:
            print(f"✅ Found {len(tables)} tables: {', '.join(tables)}\n")
            db_context["tables"] = tables
        else:
            print("⚠️  No tables found, using empty schema\n")
    except ImportError:
        print("⚠️  psycopg2 not installed, skipping table discovery")
        print("   Install with: pip install psycopg2-binary\n")
    except Exception as e:
        print(f"⚠️  Could not connect: {e}\n")
    
    # Test each query
    print("=" * 60)
    print("Testing Natural Language to SQL Conversion")
    print("=" * 60)
    
    for i, nl_query in enumerate(test_queries, 1):
        print(f"\n[Test {i}/{len(test_queries)}]")
        print(f"📝 Natural Language: \"{nl_query}\"")
        
        # Build prompt
        prompt = build_prompt(nl_query, db_context.get("tables", []))
        
        # Call Gemini
        print("🤖 Converting with AI...")
        sql = test_with_gemini(prompt, api_key)
        
        if sql:
            print(f"✅ Generated SQL:\n{sql}")
        else:
            print("❌ Failed to generate query")
        
        print("-" * 60)
    
    print("\n✨ Testing complete!")
