import web
import os
import markdown
from datetime import datetime
import re

# Define URLs and their corresponding classes
urls = (
    '/', 'Index',
    '/post/(.*)', 'Post'
)

# Directory where markdown files are stored
POSTS_DIR = 'posts'

# Function to read posts from markdown files
def get_posts():
    posts = []
    for filename in os.listdir(POSTS_DIR):
        if filename.endswith('.md'):
            filepath = os.path.join(POSTS_DIR, filename)
            with open(filepath, 'r') as f:
                # Read the first line as the title
                title = f.readline().strip()
                # Read the rest of the file as content
                content = f.read()
                html_content = markdown.markdown(content)
                # Parse the date from the filename in YYYYMMDD format
                date_str = filename.split('_')[0]
                date = datetime.strptime(date_str, '%Y%m%d').strftime('%Y-%m-%d')
                # Generate a slug from the title
                slug = re.sub(r'[^a-zA-Z0-9]+', '-', title.lower()).strip('-')
                posts.append({'title': title, 'content': html_content, 'date': date, 'slug': slug})
    # Sort posts by date, newest first
    posts.sort(key=lambda x: x['date'], reverse=True)
    return posts

# Define the Index class to handle the homepage
class Index:
    def GET(self):
        posts = get_posts()
        html = "<h1>My Blog</h1><ul>"
        for post in posts:
            html += f'<li><a href="/post/{post["slug"]}">{post["title"]} - {post["date"]}</a></li>'
        html += "</ul>"
        return html

# Define the Post class to handle individual blog posts
class Post:
    def GET(self, slug):
        posts = get_posts()
        post = next((p for p in posts if p['slug'] == slug), None)
        if post:
            return f"<h1>{post['title']}</h1><p>{post['content']}</p><a href='/'>Back to home</a>"
        else:
            return "Post not found."

# Create the application
app = web.application(urls, globals())

if __name__ == "__main__":
    app.run()
